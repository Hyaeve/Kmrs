//! Outermost middleware: runs the rest of the stack on the blocking pool. Handlers call
//! synchronous rusqlite code and r2d2 checkout blocks the calling thread while waiting
//! for a pool connection — on a runtime worker that starves the scheduler (a burst of
//! homepage requests parks every worker). The blocking pool plays Tomcat's thread role.

use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

pub async fn offload_middleware(request: Request, next: Next) -> Response {
    match tokio::task::spawn_blocking(move || {
        tokio::runtime::Handle::current().block_on(next.run(request))
    })
    .await
    {
        Ok(response) => response,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::get, Router};
    use tower::ServiceExt;

    async fn handler_thread_id(
        with_offload: bool,
    ) -> (std::thread::ThreadId, std::thread::ThreadId) {
        let (tx, rx) = std::sync::mpsc::channel();
        let route = Router::new().route(
            "/",
            get(move || {
                let tx = tx.clone();
                async move {
                    tx.send(std::thread::current().id()).unwrap();
                    "ok"
                }
            }),
        );
        let app = if with_offload {
            route.layer(axum::middleware::from_fn(offload_middleware))
        } else {
            route
        };
        let driver = std::thread::current().id();
        let _ = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        (rx.recv().unwrap(), driver)
    }

    #[tokio::test]
    async fn handler_runs_off_the_driving_thread() {
        let (handler, driver) = handler_thread_id(true).await;
        assert_ne!(handler, driver);
    }

    // baseline proving the discriminator: without the middleware the handler is
    // polled inline on the driving thread
    #[tokio::test]
    async fn without_middleware_runs_on_the_driving_thread() {
        let (handler, driver) = handler_thread_id(false).await;
        assert_eq!(handler, driver);
    }
}
