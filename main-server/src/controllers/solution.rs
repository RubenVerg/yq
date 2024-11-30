use axum::{extract::Path, http::StatusCode, response::Redirect, Extension};
use common::{langs::LANGS, RunLangOutput};
use serde::Serialize;
use sqlx::{query_scalar, types::time::OffsetDateTime, PgPool};

use crate::{
    auto_output_format::{AutoInput, AutoOutputFormat, Format},
    error::Error,
    models::{
        account::Account,
        challenge::ChallengeWithAuthorInfo,
        solutions::{Code, LeaderboardEntry, NewSolution},
    },
    slug::Slug,
    test_solution::test_solution,
};

#[derive(Serialize)]
pub struct AllSolutionsOutput {
    challenge: ChallengeWithAuthorInfo,
    leaderboard: Vec<LeaderboardEntry>,
    tests: Option<RunLangOutput>,
    code: Option<String>,
    previous_solution_invalid: bool,
}

pub async fn all_solutions(
    Path((challenge_id, _slug, language_name)): Path<(i32, String, String)>,
    format: Format,
    account: Option<Account>,
    Extension(pool): Extension<PgPool>,
) -> Result<AutoOutputFormat<AllSolutionsOutput>, Error> {
    let leaderboard = LeaderboardEntry::get_leadeboard_for_challenge_and_language(
        &pool,
        challenge_id,
        &language_name,
    )
    .await;

    let challenge = ChallengeWithAuthorInfo::get_by_id(&pool, challenge_id)
        .await?
        .ok_or(Error::NotFound)?;
    let code = match account {
        Some(account) => {
            Code::get_best_code_for_user(&pool, account.id, challenge_id, &language_name).await
        }
        None => None,
    };

    Ok(AutoOutputFormat::new(
        AllSolutionsOutput {
            challenge,
            leaderboard,
            tests: None,
            previous_solution_invalid: code.as_ref().is_some_and(|e| !e.valid),
            code: code.map(|d| d.code),
        },
        "challenge.html.jinja",
        format,
    ))
}

pub async fn challenge_redirect(
    Path(id): Path<i32>,
    account: Option<Account>,
    pool: Extension<PgPool>,
) -> Result<Redirect, Error> {
    challenge_redirect_no_slug(Path((id, None)), account, pool).await
}

pub async fn challenge_redirect_with_slug(
    Path((id, _slug)): Path<(i32, String)>,
    account: Option<Account>,
    pool: Extension<PgPool>,
) -> Result<Redirect, Error> {
    challenge_redirect_no_slug(Path((id, None)), account, pool).await
}

pub async fn challenge_redirect_no_slug(
    Path((id, language)): Path<(i32, Option<String>)>,
    account: Option<Account>,
    Extension(pool): Extension<PgPool>,
) -> Result<Redirect, Error> {
    let language = match language.as_ref() {
        Some(language) => language.as_str(),
        None => match account.as_ref() {
            Some(account) => account.preferred_language.as_str(),
            None => "python",
        },
    };

    let Some(slug) = query_scalar!("SELECT name FROM challenges WHERE id=$1", id)
        .fetch_optional(&pool)
        .await
        .map_err(Error::Database)?
    else {
        return Err(Error::NotFound);
    };

    Ok(Redirect::permanent(&format!(
        "/challenge/{id}/{}/solve/{language}",
        Slug(&slug)
    )))
}

pub async fn new_solution(
    Path((challenge_id, _slug, language_name)): Path<(i32, String, String)>,
    account: Account,
    Extension(pool): Extension<PgPool>,
    format: Format,
    AutoInput(solution): AutoInput<NewSolution>,
) -> Result<AutoOutputFormat<AllSolutionsOutput>, Error> {
    let challenge = ChallengeWithAuthorInfo::get_by_id(&pool, challenge_id)
        .await?
        .ok_or(Error::NotFound)
        .unwrap();

    let version = LANGS
        .iter()
        .find(|i| i.name == language_name)
        .ok_or(Error::NotFound)?
        .latest_version;

    let test_result = test_solution(
        &solution.code,
        &language_name,
        version,
        &challenge.challenge.challenge.judge,
    )
    .await?;
    let previous_code =
        Code::get_best_code_for_user(&pool, account.id, challenge_id, &language_name).await;

    let previous_solution_invalid = previous_code.as_ref().is_some_and(|e| !e.valid);

    let status = if test_result.tests.pass {
        // Currently the web browser turns all line breaks into "\r\n" when a solution
        // is submitted. This should eventually be fixed in the frontend, but for now
        // we just replace "\r\n" with "\n" when calculating the score to make it match
        // the byte counter in the editor.
        // Related: https://github.com/mousetail/Byte-Heist/issues/34
        let new_score = (solution.code.len() - solution.code.matches("\r\n").count()) as i32;

        match previous_code {
            None => {
                sqlx::query!(
                    "INSERT INTO solutions (language, version, challenge, code, author, score, last_improved_date) values ($1, $2, $3, $4, $5, $6, $7)",
                    language_name,
                    version,
                    challenge_id,
                    solution.code,
                    account.id,
                    new_score,
                    OffsetDateTime::now_utc()
                )
                .execute(&pool)
                .await
                .map_err(|_| Error::ServerError)?;

                StatusCode::CREATED
            }
            Some(w) if
                // Always replace an invalid solution
                !w.valid
                // Replace a solution if the score is better
                || w.score >= new_score => {
                sqlx::query!(
                    "UPDATE solutions SET 
                        code=$1,
                        score=$2,
                        valid=true,
                        validated_at=now(),
                        last_improved_date=$3
                    WHERE id=$4",
                    solution.code,
                    new_score,
                    if new_score < w.score {
                        OffsetDateTime::now_utc()
                    } else {
                        w.last_improved_date
                    },
                    w.id
                )
                .execute(&pool)
                .await
                .map_err(|_| Error::ServerError)?;

                StatusCode::CREATED
            }
            Some(_) => StatusCode::OK,
        }
    } else {
        StatusCode::BAD_REQUEST
    };
    Ok(AutoOutputFormat::new(
        AllSolutionsOutput {
            challenge,
            leaderboard: LeaderboardEntry::get_leadeboard_for_challenge_and_language(
                &pool,
                challenge_id,
                &language_name,
            )
            .await,
            tests: Some(test_result),
            code: Some(solution.code),
            previous_solution_invalid,
        },
        "challenge.html.jinja",
        format,
    )
    .with_status(status))
}
