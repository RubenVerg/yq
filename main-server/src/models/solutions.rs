use serde::{Deserialize, Serialize};
use sqlx::{query_as, query_scalar, PgPool};
use tower_sessions::cookie::time::OffsetDateTime;

#[derive(sqlx::FromRow, Deserialize, Serialize)]
pub struct NewSolution {
    pub code: String,
}

#[derive(sqlx::FromRow, Deserialize, Serialize)]
pub struct Solution {
    pub id: i32,
    pub language: String,
    pub version: String,
    pub challenge: i32,
    #[sqlx(flatten)]
    pub solution: NewSolution,
    pub author: i32,
    pub score: i32,
}

#[derive(Serialize)]
pub struct Code {
    pub code: String,
    pub score: i32,
    pub id: i32,
    pub valid: bool,
    pub last_improved_date: OffsetDateTime,
}

impl Code {
    pub async fn get_best_code_for_user(
        pool: &PgPool,
        account: i32,
        challenge: i32,
        language: &str,
    ) -> Option<Code> {
        sqlx::query_as!(
            Code,
            r#"
                SELECT code, score, id, valid, last_improved_date from solutions
                WHERE author=$1 AND challenge=$2 AND language=$3
                ORDER BY score ASC
                LIMIT 1
            "#,
            account,
            challenge,
            language
        )
        .fetch_optional(pool)
        .await
        .expect("Database connection error")
    }
}

#[derive(sqlx::FromRow, Deserialize, Serialize)]
pub struct LeaderboardEntry {
    pub id: i32,
    pub author_id: i32,
    pub author_name: String,
    pub author_avatar: String,
    pub score: i32,
}

impl LeaderboardEntry {
    pub async fn get_leadeboard_for_challenge_and_language(
        pool: &PgPool,
        challenge_id: i32,
        language: &str,
    ) -> Vec<Self> {
        sqlx::query_as!(
            LeaderboardEntry,
            "
            SELECT
                solutions.id as id,
                solutions.author as author_id,
                accounts.username as author_name,
                accounts.avatar as author_avatar,
                score FROM solutions
            LEFT JOIN accounts ON solutions.author = accounts.id
            WHERE solutions.challenge=$1 AND solutions.language=$2 AND valid=true
            ORDER BY solutions.score ASC, last_improved_date ASC
            ",
            challenge_id,
            language
        )
        .fetch_all(pool)
        .await
        .unwrap()
    }
}

#[derive(Serialize)]
pub struct InvalidatedSolution {
    language: String,
    challenge_id: i32,
    challenge_name: String,
}

impl InvalidatedSolution {
    pub async fn get_invalidated_solutions_for_user(
        user: i32,
        pool: &PgPool,
    ) -> Result<Vec<InvalidatedSolution>, sqlx::Error> {
        let result = query_as!(
            InvalidatedSolution,
            "SELECT solutions.language, challenges.id as challenge_id, challenges.name as challenge_name
            FROM solutions LEFT JOIN challenges ON solutions.challenge = challenges.id
            WHERE solutions.valid = false AND solutions.author = $1",
            user
        ).fetch_all(pool).await?;

        Ok(result)
    }

    pub async fn invalidated_solution_exists(
        user: i32,
        pool: &PgPool,
    ) -> Result<bool, sqlx::Error> {
        Ok(query_scalar!(
            "SELECT EXISTS (SELECT * FROM solutions WHERE valid=false AND author=$1)",
            user
        )
        .fetch_one(pool)
        .await?
        .unwrap())
    }
}
