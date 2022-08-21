use std::collections::HashMap;

use anyhow::Context;
use askama::Template;
use axum::{
    body::{boxed, BoxBody, Empty},
    extract::Query,
    http::{Response, StatusCode},
    Extension, Form,
};

use idlib::Authorize;

use serde::Deserialize;

use rusqlite::params;
use time::OffsetDateTime;
use tokio_rusqlite::Connection;

use crate::{
    audit::{self, AuditAction},
    error::Error,
    into_response, Service, Services,
};

struct Link {
    key: String,
    created_by: String,
    created_at: OffsetDateTime,
    used_by: Option<String>,
    used_at: Option<OffsetDateTime>,
}

#[derive(Template)]
#[template(path = "invite.html")]
struct InvitePageTemplate<'a> {
    links: Vec<Link>,
    services: &'a HashMap<String, Service>,
    error: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct InviteParams {
    error: Option<String>,
}

async fn get_users(db: &Connection) -> anyhow::Result<Vec<String>> {
    db.call(|conn| {
        let mut stmt = conn
            .prepare("SELECT username FROM users")
            .context("Failed to prepare statement")?;
        let users = stmt
            .query_map(params![], |row| Ok(row.get::<_, String>(0).unwrap()))
            .context("Failed to query users")?
            .collect::<Result<Vec<String>, rusqlite::Error>>()
            .context("Failed to collect users")?;

        Ok(users)
    })
    .await
}

pub(crate) async fn page(
    Authorize(_): Authorize<"invite", "read">,
    Query(params): Query<InviteParams>,
    Extension(db): Extension<Connection>,
    Extension(Services(services)): Extension<Services>,
) -> Result<Response<BoxBody>, Error> {
    let links = get_links(db).await?;

    let template = InvitePageTemplate {
        links,
        services: &services,
        error: params.error,
    };

    Ok(into_response(&template, "html"))
}

#[derive(Deserialize)]
struct DbLinkInfo {
    key: String,
    created_by: String,
    created_at: i64,
    used_by: Option<String>,
    used_at: Option<i64>,
}

async fn get_links(db: Connection) -> anyhow::Result<Vec<Link>> {
    db.call(move |conn| {
        let mut stmt = conn
            .prepare(
                "SELECT \
                ui.\"key\", \
                ui.created_by, \
                ui.created_at, \
                u.username AS used_by, \
                u.created_at AS used_at \
            FROM user_invites ui \
            LEFT OUTER JOIN \
                users u \
            ON \
                ui.\"key\" = u.invite_key \
            ORDER BY ui.created_at DESC",
            )
            .context("Failed to prepare link statement")?;

        let links = stmt
            .query_map(params![], |row| {
                let info = serde_rusqlite::from_row::<DbLinkInfo>(row).unwrap();

                let link = Link {
                    key: info.key,
                    created_by: info.created_by,
                    created_at: OffsetDateTime::from_unix_timestamp(info.created_at).unwrap(),
                    used_by: info.used_by,
                    used_at: info
                        .used_at
                        .map(|at| OffsetDateTime::from_unix_timestamp(at).unwrap()),
                };

                Ok(link)
            })
            .context("Failed to query invite links")?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to collect links")?;

        Ok(links)
    })
    .await
}

#[derive(Deserialize)]
pub(crate) struct DeleteForm {
    key: String,
}

pub(crate) async fn delete_page(
    Authorize(name): Authorize<"invite", "write">,
    Form(DeleteForm { key }): Form<DeleteForm>,
    Extension(db): Extension<Connection>,
) -> Result<Response<BoxBody>, Error> {
    let redirect = match delete_invite_impl(key, db, name).await {
        Ok(()) => "/admin/invite#removed".into(),
        Err(e) => format!(
            "/admin/invite?error={}",
            urlencoding::encode(&e.to_string())
        ),
    };

    let response = Response::builder()
        .header("Location", &redirect)
        .status(StatusCode::SEE_OTHER)
        .body(boxed(Empty::new()))
        .unwrap();

    Ok(response)
}

pub(crate) async fn delete_invite_impl(
    key: String,
    db: Connection,
    name: String,
) -> Result<(), Error> {
    db.call(move |conn| {
        conn.execute(
            "DELETE FROM user_invites \
            WHERE \"key\" = ?1 AND NOT EXISTS (SELECT 1 FROM users WHERE invite_key = ?1)",
            params![&key],
        )
        .context("Failed to delete invite")?;

        audit::log(conn, AuditAction::DeleteInvite(key), &name)?;

        Ok::<(), anyhow::Error>(())
    })
    .await?;

    Ok(())
}

pub(crate) async fn create_page(
    Authorize(name): Authorize<"invite", "write">,
    Form(services): Form<Vec<(String, String)>>,
    Extension(db): Extension<Connection>,
) -> Result<Response<BoxBody>, Error> {
    let services = services
        .into_iter()
        .filter_map(|(s, v)| (v == "true").then(|| s))
        .collect::<Vec<_>>();
    let services = services.join(",");

    let redirect = match create_invite_impl(db, name, services).await {
        Ok(()) => "/admin/invite#added".into(),
        Err(e) => format!(
            "/admin/invite?error={}",
            urlencoding::encode(&e.to_string())
        ),
    };

    let response = Response::builder()
        .header("Location", &redirect)
        .status(StatusCode::SEE_OTHER)
        .body(boxed(Empty::new()))
        .unwrap();

    Ok(response)
}

pub(crate) async fn create_invite_impl(
    db: Connection,
    name: String,
    services: String,
) -> Result<(), Error> {
    db.call(move |conn| {
        let key = create_invite_key();
        let now = OffsetDateTime::now_utc().unix_timestamp();
        conn.execute(
            "INSERT INTO user_invites (key, created_by, created_at, services)
            VALUES (?1, ?2, ?3, ?4)",
            params![&key, &name, now, services],
        )
        .context("Failed to delete invite")?;

        audit::log(conn, AuditAction::CreateInvite(key), &name)?;

        Ok::<(), anyhow::Error>(())
    })
    .await?;

    Ok(())
}

fn create_invite_key() -> String {
    blob_uuid::random_blob()
}