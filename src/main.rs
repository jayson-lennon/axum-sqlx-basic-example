use axum::{http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, sync::Arc};
use tracing::{debug, info};

use sqlx::{sqlite::SqlitePoolOptions, Sqlite};

#[derive(Debug)]
struct NonClonableStruct;

#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long, env = "DATABASE_URL", default_value = "sqlite:data.db")]
    db_url: String,
}

// global program state will be managed by axum
#[derive(Clone, Debug)]
pub struct ProgramState {
    // Pool is internally behind an `Arc` (which makes it clonable), so we are ok:
    db_pool: sqlx::Pool<Sqlite>,
    // `String` implements `Clone`, so we are ok:
    some_other_data: String,
    // `NonCloneableStruct` does _not_ implement `Clone`, so we need to wrap it in an `Arc`:
    more_state: Arc<NonClonableStruct>,
}

#[tokio::main]
async fn main() {
    // initialize tracing
    tracing::subscriber::set_global_default(tracing_subscriber::fmt::Subscriber::new())
        .expect("setting tracing default failed");

    // get CLI args
    let args = Args::parse();

    // connect to database
    let db_pool = SqlitePoolOptions::new()
        .connect(&args.db_url)
        .await
        .expect("failed to connect to database");
    info!(db_url = args.db_url, "connected to database");

    // create new program state
    let state = ProgramState {
        db_pool,
        some_other_data: String::from("hello"),
        // manually wrap this in Arc because it didn't derive Clone
        more_state: Arc::new(NonClonableStruct),
    };
    info!("state created");

    // build our application with a route
    let app = Router::new()
        // `GET /`
        .route("/", get(root))
        .route("/hit", get(route::root))
        // `GET /hit/<anything>`
        .route("/hit/:url", get(route::hit))
        // move the program state into the axum app
        .with_state(state);
    info!("app built");

    // run our app with hyper
    // `axum::Server` is a re-export of `hyper::Server`
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    info!("listening on {}", addr);
    axum::Server::bind(&addr)
        // set up this server with our app from above
        .serve(app.into_make_service())
        .await
        .unwrap();
}

// separate routes into own module
mod route {
    use axum::extract::{Path, State};

    use super::*;

    pub async fn root() -> impl IntoResponse {
        "Navigate to `/hit/foo` to increment the hit count for `foo`"
    }

    // when the user navigates to this route (via /hit), axum will:
    // 1. Extract the Path from the address. We destructure it into the `url` variable
    // 2. Provide the program state using State. We destructire it into the `state` variable.
    //
    // Internally, it goes something like this:
    // 1. User visits /hit
    // 2. Axum figures out that it needs to call this function
    // 3. Axum sees that the parameters require Path and State. Both of these are Axum extractors
    //    which can pull information from different things.
    // 4. Axum provides the Path and State structure to us. State is probably behind an Arc,
    //    so we are accessing the State that exists inside Axum.
    // 5. We destructure them immediately in the parameters, so we don't have to do so in the
    //    function body
    // 6. We can now do whatever we want with them in the function body
    // 7. Function ends. Path is dropped. State is dropped. State is probably an Arc,
    //    so the State still exists within Axum because we used .with_state() when
    //    we created the app.
    pub async fn hit(
        Path(url): Path<String>,           // access to the URL
        State(state): State<ProgramState>, // access to entire program state
    ) -> impl IntoResponse {
        info!(url = url, "hit");
        // get the db pool from the program state, and then get a connection from it
        let connection = state.db_pool.acquire().await.unwrap();
        // pass connection to increase_hit_count so we can update the hits
        let hits = query::increase_hit_count(&url, connection).await.unwrap();
        info!(url = url, hits = hits, "total hits");
        // return the total hits
        format!("{hits}")
    }
}

// basic handler that responds with a static string
async fn root() -> &'static str {
    "Navigate to `/hit/foo` to increment the hit count for `foo`"
}

// separate the queries into own module
mod query {
    use sqlx::{pool::PoolConnection, Sqlite};
    use tracing::info;

    pub async fn increase_hit_count(
        target: &str,
        mut connection: PoolConnection<Sqlite>,
    ) -> Result<i64, color_eyre::eyre::Report> {
        // upsert hits
        sqlx::query!(
            "INSERT INTO hits(target, count) VALUES(?, 1)
         ON CONFLICT(target) DO UPDATE SET
         count=count + 1",
            target
        )
        .execute(&mut connection)
        .await?;

        info!(url = target, "hit count incremented");

        // query the current count
        Ok(
            sqlx::query!("SELECT count FROM hits WHERE target = ?", target)
                .fetch_one(&mut connection)
                .await?
                .count,
        )
    }
}
