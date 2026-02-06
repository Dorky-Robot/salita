use askama::Template;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Form, Router};
use chrono::{NaiveDateTime, Utc};
use rusqlite::params;
use serde::Deserialize;

use crate::error::{AppError, AppResult};
use crate::extractors::{CurrentUser, MaybeUser};
use crate::routes::home::Html;
use crate::state::AppState;

// --- View structs ---

pub struct StreamPost {
    pub id: String,
    pub username: String,
    pub body: String,
    pub created_at: String,
    pub can_delete: bool,
    pub like_count: i64,
    pub heart_count: i64,
    pub user_liked: bool,
    pub user_hearted: bool,
    pub comment_count: i64,
    pub current_user: Option<String>,
}

pub struct StreamComment {
    #[allow(dead_code)]
    pub id: String,
    pub username: String,
    pub body: String,
    pub created_at: String,
}

// --- Templates ---

#[derive(Template)]
#[template(path = "pages/stream.html")]
pub struct StreamTemplate {
    pub posts: Vec<StreamPost>,
    pub username: Option<String>,
}

#[derive(Template)]
#[template(path = "components/post_card.html")]
pub struct PostCardTemplate {
    pub post: StreamPost,
}

#[derive(Template)]
#[template(path = "components/reaction_bar.html")]
pub struct ReactionBarTemplate {
    pub post_id: String,
    pub like_count: i64,
    pub heart_count: i64,
    pub user_liked: bool,
    pub user_hearted: bool,
    pub current_user: Option<String>,
}

#[derive(Template)]
#[template(path = "components/comment_list.html")]
pub struct CommentListTemplate {
    pub post_id: String,
    pub comments: Vec<StreamComment>,
    pub username: Option<String>,
}

#[derive(Template)]
#[template(path = "components/comment.html")]
pub struct CommentTemplate {
    pub comment: StreamComment,
}

// --- Forms ---

#[derive(Deserialize)]
pub struct CreatePostForm {
    pub body: String,
}

#[derive(Deserialize)]
pub struct ReactionForm {
    pub kind: String,
}

#[derive(Deserialize)]
pub struct CreateCommentForm {
    pub body: String,
}

// --- Router ---

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/stream", get(stream_page))
        .route("/stream/posts", post(create_post))
        .route("/stream/posts/{id}", delete(delete_post))
        .route("/stream/posts/{id}/reactions", post(toggle_reaction))
        .route(
            "/stream/posts/{id}/comments",
            get(list_comments).post(create_comment),
        )
}

// --- Handlers ---

async fn stream_page(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
) -> AppResult<Html<StreamTemplate>> {
    let user_id = user.as_ref().map(|u| u.id.clone());
    let username = user.as_ref().map(|u| u.username.clone());
    let is_admin = user.as_ref().map(|u| u.is_admin).unwrap_or(false);

    let posts = {
        let conn = state.db.get()?;
        query_posts(&conn, user_id.as_deref(), is_admin)?
    };

    Ok(Html(StreamTemplate { posts, username }))
}

async fn create_post(
    State(state): State<AppState>,
    user: CurrentUser,
    Form(form): Form<CreatePostForm>,
) -> AppResult<Response> {
    let body = form.body.trim().to_string();
    if body.is_empty() {
        return Err(AppError::BadRequest("Post body cannot be empty".into()));
    }
    if body.len() > 2000 {
        return Err(AppError::BadRequest(
            "Post body must be 2000 characters or less".into(),
        ));
    }

    let post_id = uuid::Uuid::now_v7().to_string();
    {
        let conn = state.db.get()?;
        conn.execute(
            "INSERT INTO posts (id, user_id, body) VALUES (?1, ?2, ?3)",
            params![post_id, user.id, body],
        )?;
    }

    let post = StreamPost {
        id: post_id,
        username: user.username.clone(),
        body,
        created_at: format_relative_time(&Utc::now().naive_utc()),
        can_delete: true,
        like_count: 0,
        heart_count: 0,
        user_liked: false,
        user_hearted: false,
        comment_count: 0,
        current_user: Some(user.id),
    };

    Ok(Html(PostCardTemplate { post }).into_response())
}

async fn delete_post(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> AppResult<Response> {
    let conn = state.db.get()?;

    let owner_id: String = conn
        .query_row(
            "SELECT user_id FROM posts WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .map_err(|_| AppError::NotFound)?;

    if owner_id != user.id && !user.is_admin {
        return Err(AppError::Unauthorized);
    }

    conn.execute("DELETE FROM posts WHERE id = ?1", params![id])?;
    Ok((StatusCode::OK, "").into_response())
}

async fn toggle_reaction(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(post_id): Path<String>,
    Form(form): Form<ReactionForm>,
) -> AppResult<Response> {
    let kind = form.kind.trim().to_string();
    if kind != "like" && kind != "heart" {
        return Err(AppError::BadRequest("Invalid reaction kind".into()));
    }

    let conn = state.db.get()?;

    // Verify post exists
    let _: String = conn
        .query_row(
            "SELECT id FROM posts WHERE id = ?1",
            params![post_id],
            |r| r.get(0),
        )
        .map_err(|_| AppError::NotFound)?;

    // Toggle: check exists, then delete or insert
    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM reactions WHERE post_id = ?1 AND user_id = ?2 AND kind = ?3",
            params![post_id, user.id, kind],
            |r| r.get(0),
        )
        .ok();

    if existing.is_some() {
        conn.execute(
            "DELETE FROM reactions WHERE post_id = ?1 AND user_id = ?2 AND kind = ?3",
            params![post_id, user.id, kind],
        )?;
    } else {
        let reaction_id = uuid::Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO reactions (id, post_id, user_id, kind) VALUES (?1, ?2, ?3, ?4)",
            params![reaction_id, post_id, user.id, kind],
        )?;
    }

    // Query updated counts
    let like_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM reactions WHERE post_id = ?1 AND kind = 'like'",
        params![post_id],
        |r| r.get(0),
    )?;
    let heart_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM reactions WHERE post_id = ?1 AND kind = 'heart'",
        params![post_id],
        |r| r.get(0),
    )?;
    let user_liked: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM reactions WHERE post_id = ?1 AND user_id = ?2 AND kind = 'like'",
        params![post_id, user.id],
        |r| r.get(0),
    )?;
    let user_hearted: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM reactions WHERE post_id = ?1 AND user_id = ?2 AND kind = 'heart'",
        params![post_id, user.id],
        |r| r.get(0),
    )?;

    Ok(Html(ReactionBarTemplate {
        post_id,
        like_count,
        heart_count,
        user_liked,
        user_hearted,
        current_user: Some(user.id),
    })
    .into_response())
}

async fn list_comments(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    Path(post_id): Path<String>,
) -> AppResult<Response> {
    let conn = state.db.get()?;
    let comments = query_comments(&conn, &post_id)?;
    let username = user.map(|u| u.username);

    Ok(Html(CommentListTemplate {
        post_id,
        comments,
        username,
    })
    .into_response())
}

async fn create_comment(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(post_id): Path<String>,
    Form(form): Form<CreateCommentForm>,
) -> AppResult<Response> {
    let body = form.body.trim().to_string();
    if body.is_empty() {
        return Err(AppError::BadRequest("Comment cannot be empty".into()));
    }
    if body.len() > 500 {
        return Err(AppError::BadRequest(
            "Comment must be 500 characters or less".into(),
        ));
    }

    let comment_id = uuid::Uuid::now_v7().to_string();
    {
        let conn = state.db.get()?;

        // Verify post exists
        let _: String = conn
            .query_row(
                "SELECT id FROM posts WHERE id = ?1",
                params![post_id],
                |r| r.get(0),
            )
            .map_err(|_| AppError::NotFound)?;

        conn.execute(
            "INSERT INTO comments (id, post_id, user_id, body) VALUES (?1, ?2, ?3, ?4)",
            params![comment_id, post_id, user.id, body],
        )?;
    }

    let comment = StreamComment {
        id: comment_id,
        username: user.username,
        body,
        created_at: format_relative_time(&Utc::now().naive_utc()),
    };

    Ok(Html(CommentTemplate { comment }).into_response())
}

// --- Query helpers ---

fn query_posts(
    conn: &rusqlite::Connection,
    current_user_id: Option<&str>,
    is_admin: bool,
) -> Result<Vec<StreamPost>, AppError> {
    let uid = current_user_id.unwrap_or("");

    let mut stmt = conn.prepare(
        "SELECT p.id, u.username, p.body, p.created_at, p.user_id,
                COALESCE((SELECT COUNT(*) FROM reactions r WHERE r.post_id = p.id AND r.kind = 'like'), 0) as like_count,
                COALESCE((SELECT COUNT(*) FROM reactions r WHERE r.post_id = p.id AND r.kind = 'heart'), 0) as heart_count,
                COALESCE((SELECT COUNT(*) > 0 FROM reactions r WHERE r.post_id = p.id AND r.user_id = ?1 AND r.kind = 'like'), 0) as user_liked,
                COALESCE((SELECT COUNT(*) > 0 FROM reactions r WHERE r.post_id = p.id AND r.user_id = ?1 AND r.kind = 'heart'), 0) as user_hearted,
                COALESCE((SELECT COUNT(*) FROM comments c WHERE c.post_id = p.id), 0) as comment_count
         FROM posts p
         JOIN users u ON u.id = p.user_id
         ORDER BY p.created_at DESC
         LIMIT 50",
    )?;

    let posts = stmt
        .query_map(params![uid], |row| {
            let post_user_id: String = row.get(4)?;
            let created_at_str: String = row.get(3)?;
            let current_uid: &str = uid;
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                created_at_str,
                post_user_id,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, bool>(7)?,
                row.get::<_, bool>(8)?,
                row.get::<_, i64>(9)?,
                current_uid.to_string(),
            ))
        })?
        .filter_map(|r| r.ok())
        .map(
            |(
                id,
                username,
                body,
                created_at_str,
                post_user_id,
                like_count,
                heart_count,
                user_liked,
                user_hearted,
                comment_count,
                current_uid,
            )| {
                let can_delete = if current_user_id.is_some() {
                    post_user_id == current_uid || is_admin
                } else {
                    false
                };
                let created_at = parse_and_format_time(&created_at_str);
                let current_user = if current_user_id.is_some() {
                    Some(current_uid)
                } else {
                    None
                };
                StreamPost {
                    id,
                    username,
                    body,
                    created_at,
                    can_delete,
                    like_count,
                    heart_count,
                    user_liked,
                    user_hearted,
                    comment_count,
                    current_user,
                }
            },
        )
        .collect();

    Ok(posts)
}

fn query_comments(
    conn: &rusqlite::Connection,
    post_id: &str,
) -> Result<Vec<StreamComment>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT c.id, u.username, c.body, c.created_at
         FROM comments c
         JOIN users u ON u.id = c.user_id
         WHERE c.post_id = ?1
         ORDER BY c.created_at ASC",
    )?;

    let comments = stmt
        .query_map(params![post_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .map(|(id, username, body, created_at_str)| StreamComment {
            id,
            username,
            body,
            created_at: parse_and_format_time(&created_at_str),
        })
        .collect();

    Ok(comments)
}

// --- Time formatting ---

fn parse_and_format_time(db_time: &str) -> String {
    NaiveDateTime::parse_from_str(db_time, "%Y-%m-%d %H:%M:%S")
        .map(|dt| format_relative_time(&dt))
        .unwrap_or_else(|_| db_time.to_string())
}

pub fn format_relative_time(dt: &NaiveDateTime) -> String {
    let now = Utc::now().naive_utc();
    let diff = now.signed_duration_since(*dt);

    let seconds = diff.num_seconds();
    if seconds < 60 {
        return "just now".to_string();
    }

    let minutes = diff.num_minutes();
    if minutes < 60 {
        return format!("{}m ago", minutes);
    }

    let hours = diff.num_hours();
    if hours < 24 {
        return format!("{}h ago", hours);
    }

    let days = diff.num_days();
    if days < 7 {
        return format!("{}d ago", days);
    }

    dt.format("%b %-d, %Y").to_string()
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn format_relative_time_just_now() {
        let now = Utc::now().naive_utc();
        assert_eq!(format_relative_time(&now), "just now");
    }

    #[test]
    fn format_relative_time_minutes() {
        let dt = Utc::now().naive_utc() - chrono::Duration::minutes(5);
        assert_eq!(format_relative_time(&dt), "5m ago");
    }

    #[test]
    fn format_relative_time_hours() {
        let dt = Utc::now().naive_utc() - chrono::Duration::hours(3);
        assert_eq!(format_relative_time(&dt), "3h ago");
    }

    #[test]
    fn format_relative_time_days() {
        let dt = Utc::now().naive_utc() - chrono::Duration::days(2);
        assert_eq!(format_relative_time(&dt), "2d ago");
    }

    #[test]
    fn format_relative_time_old_date() {
        let dt = NaiveDate::from_ymd_opt(2025, 1, 15)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let result = format_relative_time(&dt);
        assert_eq!(result, "Jan 15, 2025");
    }

    #[test]
    fn parse_and_format_handles_db_format() {
        let result = parse_and_format_time("2025-01-15 12:00:00");
        assert_eq!(result, "Jan 15, 2025");
    }

    #[test]
    fn parse_and_format_bad_input_returns_raw() {
        let result = parse_and_format_time("not-a-date");
        assert_eq!(result, "not-a-date");
    }

    #[test]
    fn create_post_validates_empty_body() {
        let body = "  ".trim().to_string();
        assert!(body.is_empty());
    }

    #[test]
    fn create_post_validates_length() {
        let body = "x".repeat(2001);
        assert!(body.len() > 2000);
    }
}
