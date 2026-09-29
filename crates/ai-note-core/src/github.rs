//! GitHub REST 서비스 (M1 구현 시작 — 계획 §4 `github`, A3/A7/R3).
//!
//! ghp(classic, `repo` 스코프)를 보유한 앱이 저장소를 **자동 생성**한다
//! (`private: true` — A3). PAT 검증·접근 거부 처리를 포함한다.
//! 승인 상태는 앱 DB가 소유(GitHub 네이티브 required-reviews 불가 — 단일 신원).

use octocrab::Octocrab;
use serde::Serialize;

#[derive(thiserror::Error, Debug)]
pub enum GithubError {
    #[error("GitHub 연결에 문제가 생겼어요. 인터넷 상태를 확인해 주세요")]
    Network(#[from] octocrab::Error),
    #[error("초대장의 권한이 부족해요. 저장소 만들기 권한이 있는지 확인해 주세요")]
    InsufficientScope,
    #[error("{0}")]
    Other(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum RepoOutcome {
    /// 새 저장소를 만들었다(앱이 초기화해야 함).
    Created,
    /// 이미 존재했다(재사용 — 초대장 재사용 정책 F26과 정합).
    Existing,
}

#[derive(Debug, Clone, Serialize)]
pub struct UserInfo {
    pub login: String,
}

pub struct GithubService {
    client: Octocrab,
}

#[derive(Serialize)]
struct CreateRepoBody {
    name: String,
    #[serde(rename = "private")]
    is_private: bool,
    description: String,
    auto_init: bool,
}

impl GithubService {
    /// ghp로 서비스를 만든다. 토큰은 여기서만 쓰이고 디스크에 기록되지 않는다.
    pub fn new(pat: &str) -> Result<Self, GithubError> {
        Self::new_at(pat, None)
    }

    /// base URL 지정 버전(wiremock 통합 테스트용). None이면 github.com.
    pub fn new_at(pat: &str, base_url: Option<&str>) -> Result<Self, GithubError> {
        let mut builder = Octocrab::builder().personal_token(pat.to_string());
        if let Some(base) = base_url {
            builder = builder
                .base_uri(base)
                .map_err(|e| GithubError::Other(format!("잘못된 연결 주소: {e}")))?;
        }
        let client = builder
            .build()
            .map_err(|e| GithubError::Other(format!("클라이언트 생성 실패: {e}")))?;
        Ok(Self { client })
    }

    /// PAT 검증 — GET /user. 미구독/만료 토큰·scope 부족을 여기서 가린다.
    ///
    /// typed 모델 대신 raw JSON으로 필요 필드만 읽는다(모델 필수 필드
    /// 드리프트에 영향받지 않게 — wiremock 목도 최소 필드로 유지).
    pub async fn verify_token(&self) -> Result<UserInfo, GithubError> {
        let resp = self.client._get("/user").await.map_err(github_err)?;
        let status = resp.status();
        if status.as_u16() == 401 || status.as_u16() == 403 || status.as_u16() == 404 {
            return Err(GithubError::InsufficientScope);
        }
        let bytes = http_body_util::BodyExt::collect(resp.into_body())
            .await
            .map_err(|e| GithubError::Other(format!("응답 수신 실패: {e}")))?
            .to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|e| GithubError::Other(format!("응답 해석 실패: {e}")))?;
        let login = body
            .get("login")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        if login.is_empty() {
            return Err(GithubError::Other("사용자 정보가 비어 있어요".into()));
        }
        Ok(UserInfo { login })
    }

    /// 저장소 자동 생성(A3) — private 고정(A7/Q3). 이미 있으면 Existing.
    pub async fn ensure_repo(&self, name: &str) -> Result<RepoOutcome, GithubError> {
        let result = self
            .client
            ._post(
                "/user/repos",
                Some(&CreateRepoBody {
                    name: name.to_string(),
                    is_private: true,
                    description: "AI Note 팀 볼트".into(),
                    auto_init: false,
                }),
            )
            .await;
        let resp = result.map_err(github_err)?;
        match resp.status().as_u16() {
            200..=202 => Ok(RepoOutcome::Created),
            // 422: 이름 충돌 = 이미 존재 → 재사용(F26 재사용 정책)
            422 => Ok(RepoOutcome::Existing),
            401 | 403 | 404 => Err(GithubError::InsufficientScope),
            other => Err(GithubError::Other(format!("저장소를 준비하지 못했어요 (code {other})"))),
        }
    }
}

/// octocrab 오류 → 친화 분류(401/403/404 = 권한 문제 안내).
fn github_err(e: octocrab::Error) -> GithubError {
    if let octocrab::Error::GitHub { source, .. } = &e {
        match source.status_code.as_u16() {
            401 | 403 | 404 => return GithubError::InsufficientScope,
            _ => {}
        }
    }
    GithubError::Network(e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn m1_verify_token_success() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/user"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "login": "team-ainote", "id": 1
            })))
            .mount(&server)
            .await;
        let svc = service_at(&server).await;
        let info = svc.verify_token().await.unwrap();
        assert_eq!(info.login, "team-ainote");
    }

    #[tokio::test]
    async fn m1_verify_token_rejects_bad_token() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/user"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let svc = service_at(&server).await;
        assert!(matches!(svc.verify_token().await, Err(GithubError::InsufficientScope)));
    }

    #[tokio::test]
    async fn m1_ensure_repo_creates_private() {
        let server = MockServer::start().await;
        use wiremock::matchers::body_json;
        let expected_body = serde_json::json!({
            "name": "my-team-notes", "private": true,
            "description": "AI Note 팀 볼트", "auto_init": false
        });
        Mock::given(method("POST")).and(path("/user/repos")).and(body_json(expected_body))
            .respond_with(ResponseTemplate::new(202).set_body_json(serde_json::json!({
                "full_name": "team-ainote/my-team-notes", "private": true
            })))
            .expect(1)
            .mount(&server)
            .await;
        let svc = service_at(&server).await;
        assert_eq!(svc.ensure_repo("my-team-notes").await.unwrap(), RepoOutcome::Created);
        // private:true 고정 검증 — 요청 본문 매처로 단얨
        server.verify().await;
    }

    #[tokio::test]
    async fn m1_ensure_repo_existing_reused() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/user/repos"))
            .respond_with(ResponseTemplate::new(422).set_body_json(serde_json::json!({
                "message": "name already exists on this account"
            })))
            .mount(&server)
            .await;
        let svc = service_at(&server).await;
        assert_eq!(svc.ensure_repo("my-team-notes").await.unwrap(), RepoOutcome::Existing);
    }

    #[tokio::test]
    async fn m1_ensure_repo_scope_failure_is_friendly() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/user/repos"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        let svc = service_at(&server).await;
        assert!(matches!(svc.ensure_repo("x").await, Err(GithubError::InsufficientScope)));
    }

    /// 목 서버를 가리키는 서비스(base URL 재지정)를 만든다.
    async fn service_at(server: &MockServer) -> GithubService {
        GithubService::new_at("ghp_testtoken", Some(&server.uri())).unwrap()
    }
}
