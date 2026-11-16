//! The farm's tenants and tenant requests (0111 §7; 0046 §9; 08 §1.4): `GET /tenancy`, the
//! `/tenancy/requests` routes at the farm base, and a tenant owner's invitation on the
//! tenant it is for, `/auth/invitation` (0111 §5, A2).

use axum::http::{Method, StatusCode};
use serde::Deserialize;
use serde_json::Value;

use crate::api::{CacheInfo, Fetched};
use crate::settings::{BASE, Saved, read, saved};
use crate::transport::ClientError;
use crate::{Client, Incoming};

/// A tenant in `GET /tenancy`'s list.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct TenantRow {
    /// The slug.
    pub slug: String,
    /// The issuer code.
    pub issuer: String,
    /// The host.
    #[serde(default)]
    pub host: Option<String>,
    /// The base.
    pub base: String,
    /// The wiki's name.
    pub name: String,
    /// `active`.
    pub status: String,
}

/// `GET /tenancy`: the farm, the primary tenant and the tenants.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Tenancy {
    /// The farm: `slug`, `code`, `base`.
    pub farm: Value,
    /// The primary tenant's slug.
    pub primary: Option<String>,
    /// The tenants.
    pub tenants: Vec<TenantRow>,
}

/// A refusal of a tenant request.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct RequestRefusal {
    /// The code: `ts-not-entitled`, `ts-tenant-invalid`, `ts-auth-base-required`,
    /// `ts-owner-no-login`.
    pub code: String,
    /// Why.
    #[serde(default)]
    pub why: Option<String>,
}

/// A tenant request with its state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct TenantRequest {
    /// Its ID, the offset in the instance `log`.
    pub id: u64,
    /// `pending`, `created` or `refused`.
    pub state: String,
    /// When it was filed, microseconds.
    pub time: u64,
    /// Who filed it.
    #[serde(default)]
    pub actor: Option<String>,
    /// Its public reason.
    #[serde(default)]
    pub reason: Option<String>,
    /// The slug.
    pub slug: String,
    /// The base.
    pub base: String,
    /// The wiki's name.
    #[serde(default)]
    pub name: Option<String>,
    /// The owner's account name.
    pub owner_name: String,
    /// The account whose login the owner is bound to.
    #[serde(default)]
    pub owner_login_source: Option<String>,
    /// The refusal, for a refused request.
    #[serde(default)]
    pub refusal: Option<RequestRefusal>,
}

/// What a form files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TenantAsk {
    /// The slug.
    pub slug: String,
    /// The base.
    pub base: String,
    /// The wiki's name.
    pub name: String,
    /// The owner's account name.
    pub owner_name: String,
    /// The account whose login the owner is bound to, by actor key; empty for none.
    pub owner_login_source: String,
    /// The public reason.
    pub reason: String,
    /// The CSRF token.
    pub token: String,
}

/// An invitation just issued: its code, shown once, and the address to send.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Invitation {
    /// The code.
    pub code: String,
    /// `{tenant base}/wiki/Special:CreateAccount?invite={code}`.
    pub url: String,
    /// The tenant.
    pub tenant: String,
    /// How many days it is valid.
    pub valid_days: u32,
}

/// What an open invitation is for, as `GET /auth/invitation` says.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct InvitationInfo {
    /// The wiki's name.
    pub wiki: String,
    /// The owner account's name.
    #[serde(default)]
    pub owner: Option<String>,
    /// When it expires, microseconds.
    pub expires: u64,
}

#[derive(Debug, Deserialize)]
struct Requests {
    requests: Vec<TenantRequest>,
}

#[derive(Debug, Deserialize)]
struct OneRequest {
    request: TenantRequest,
}

#[derive(Debug, Deserialize)]
struct Invited {
    invitation: Invitation,
}

#[derive(Debug, Deserialize)]
struct InvitationAnswer {
    invitation: InvitationInfo,
}

#[derive(Debug, Deserialize)]
struct Redeemed {
    invitation: RedeemedInner,
}

#[derive(Debug, Deserialize)]
struct RedeemedInner {
    username: String,
}

impl Client {
    /// `GET /tenancy`.
    ///
    /// # Errors
    ///
    /// The API's refusal, or the API away or unreadable.
    pub async fn tenancy(&self, incoming: &Incoming) -> Result<Fetched<Tenancy>, ClientError> {
        let r = self.get(incoming, &format!("{BASE}/tenancy")).await?;
        Ok(Fetched {
            value: read(&r, "tenancy")?,
            cache: CacheInfo::of(&r),
        })
    }

    /// `GET /tenancy/requests`, newest first.
    ///
    /// # Errors
    ///
    /// The API's refusal (`permissiondenied` without `ts-createtenant`), or the API away
    /// or unreadable.
    pub async fn tenant_requests(
        &self,
        incoming: &Incoming,
    ) -> Result<Vec<TenantRequest>, ClientError> {
        let r = self
            .get(incoming, &format!("{BASE}/tenancy/requests"))
            .await?;
        Ok(read::<Requests>(&r, "tenant requests")?.requests)
    }

    /// `POST /tenancy/requests`: the request filed, `pending`.
    ///
    /// # Errors
    ///
    /// The API away or unreadable; a refusal is in the `Saved`.
    pub async fn file_tenant_request(
        &self,
        incoming: &Incoming,
        ask: &TenantAsk,
    ) -> Result<Saved<TenantRequest>, ClientError> {
        let optional = |s: &str| {
            let s = s.trim();
            (!s.is_empty()).then(|| s.to_string())
        };
        let r = self
            .send_json(
                incoming,
                Method::POST,
                &format!("{BASE}/tenancy/requests"),
                &serde_json::json!({
                    "slug": ask.slug.trim(), "base": ask.base.trim(),
                    "name": optional(&ask.name), "owner_name": ask.owner_name.trim(),
                    "owner_login_source": optional(&ask.owner_login_source),
                    "reason": optional(&ask.reason), "token": ask.token,
                }),
            )
            .await?;
        if r.status == StatusCode::ACCEPTED {
            let one: OneRequest = serde_json::from_slice(&r.body)
                .map_err(|e| ClientError::Unexpected(format!("tenant request: {e}")))?;
            return Ok(Saved::Ok(one.request));
        }
        Ok(match saved::<OneRequest>(&r, "tenant request")? {
            Saved::Ok(one) => Saved::Ok(one.request),
            Saved::Refused(e) => Saved::Refused(e),
        })
    }

    /// `POST /tenancy/requests/{id}/invitation`.
    ///
    /// # Errors
    ///
    /// The API away or unreadable; a refusal is in the `Saved`.
    pub async fn invite_tenant_owner(
        &self,
        incoming: &Incoming,
        id: u64,
        token: &str,
    ) -> Result<Saved<Invitation>, ClientError> {
        let r = self
            .send_json(
                incoming,
                Method::POST,
                &format!("{BASE}/tenancy/requests/{id}/invitation"),
                &serde_json::json!({"token": token}),
            )
            .await?;
        Ok(match saved::<Invited>(&r, "invitation")? {
            Saved::Ok(i) => Saved::Ok(i.invitation),
            Saved::Refused(e) => Saved::Refused(e),
        })
    }

    /// `GET /auth/invitation?code=…`: what an open invitation is for, `None` if the code is
    /// unknown, used or expired.
    ///
    /// # Errors
    ///
    /// The API away or unreadable.
    pub async fn invitation(
        &self,
        incoming: &Incoming,
        code: &str,
    ) -> Result<Option<InvitationInfo>, ClientError> {
        let code: String = url::form_urlencoded::byte_serialize(code.as_bytes()).collect();
        let r = self
            .get(incoming, &format!("{BASE}/auth/invitation?code={code}"))
            .await?;
        if r.status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        Ok(Some(read::<InvitationAnswer>(&r, "invitation")?.invitation))
    }

    /// `POST /auth/invitation`: the owner's name, to log in with.
    ///
    /// # Errors
    ///
    /// The API away or unreadable; a refusal is in the `Saved`.
    pub async fn redeem_invitation(
        &self,
        incoming: &Incoming,
        code: &str,
        password: &str,
    ) -> Result<Saved<String>, ClientError> {
        let r = self
            .send_json(
                incoming,
                Method::POST,
                &format!("{BASE}/auth/invitation"),
                &serde_json::json!({"code": code, "password": password}),
            )
            .await?;
        Ok(match saved::<Redeemed>(&r, "invitation")? {
            Saved::Ok(i) => Saved::Ok(i.invitation.username),
            Saved::Refused(e) => Saved::Refused(e),
        })
    }
}
