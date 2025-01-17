/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs Ltd <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use std::{net::IpAddr, sync::Arc};

use common::{
    ip_to_bytes,
    listener::limiter::{ConcurrencyLimiter, InFlight},
    ConcurrencyLimiters, Server, KV_RATE_LIMIT_HTTP_ANONYMOUS, KV_RATE_LIMIT_HTTP_AUTHENTICATED,
};
use directory::Permission;
use trc::AddContext;

use common::auth::AccessToken;
use std::future::Future;

pub trait RateLimiter: Sync + Send {
    fn get_concurrency_limiter(&self, account_id: u32) -> Arc<ConcurrencyLimiters>;
    fn is_http_authenticated_request_allowed(
        &self,
        access_token: &AccessToken,
    ) -> impl Future<Output = trc::Result<InFlight>> + Send;
    fn is_http_anonymous_request_allowed(
        &self,
        addr: &IpAddr,
    ) -> impl Future<Output = trc::Result<()>> + Send;
    fn is_upload_allowed(&self, access_token: &AccessToken) -> trc::Result<InFlight>;
}

impl RateLimiter for Server {
    fn get_concurrency_limiter(&self, account_id: u32) -> Arc<ConcurrencyLimiters> {
        self.inner
            .data
            .jmap_limiter
            .get(&account_id)
            .map(|limiter| limiter.clone())
            .unwrap_or_else(|| {
                let limiter = Arc::new(ConcurrencyLimiters {
                    concurrent_requests: ConcurrencyLimiter::new(
                        self.core.jmap.request_max_concurrent,
                    ),
                    concurrent_uploads: ConcurrencyLimiter::new(
                        self.core.jmap.upload_max_concurrent,
                    ),
                });
                self.inner
                    .data
                    .jmap_limiter
                    .insert(account_id, limiter.clone());
                limiter
            })
    }

    async fn is_http_authenticated_request_allowed(
        &self,
        access_token: &AccessToken,
    ) -> trc::Result<InFlight> {
        let limiter = self.get_concurrency_limiter(access_token.primary_id());
        let is_rate_allowed = if let Some(rate) = &self.core.jmap.rate_authenticated {
            self.core
                .storage
                .lookup
                .is_rate_allowed(
                    KV_RATE_LIMIT_HTTP_AUTHENTICATED,
                    &access_token.primary_id.to_be_bytes(),
                    rate,
                    false,
                )
                .await
                .caused_by(trc::location!())?
                .is_none()
        } else {
            true
        };

        if is_rate_allowed {
            if let Some(in_flight_request) = limiter.concurrent_requests.is_allowed() {
                Ok(in_flight_request)
            } else if access_token.has_permission(Permission::UnlimitedRequests) {
                Ok(InFlight::default())
            } else {
                Err(trc::LimitEvent::ConcurrentRequest.into_err())
            }
        } else if access_token.has_permission(Permission::UnlimitedRequests) {
            Ok(InFlight::default())
        } else {
            Err(trc::LimitEvent::TooManyRequests.into_err())
        }
    }

    async fn is_http_anonymous_request_allowed(&self, addr: &IpAddr) -> trc::Result<()> {
        if let Some(rate) = &self.core.jmap.rate_anonymous {
            if !self.is_ip_allowed(addr)
                && self
                    .core
                    .storage
                    .lookup
                    .is_rate_allowed(
                        KV_RATE_LIMIT_HTTP_ANONYMOUS,
                        &ip_to_bytes(addr),
                        rate,
                        false,
                    )
                    .await
                    .caused_by(trc::location!())?
                    .is_some()
            {
                return Err(trc::LimitEvent::TooManyRequests.into_err());
            }
        }
        Ok(())
    }

    fn is_upload_allowed(&self, access_token: &AccessToken) -> trc::Result<InFlight> {
        if let Some(in_flight_request) = self
            .get_concurrency_limiter(access_token.primary_id())
            .concurrent_uploads
            .is_allowed()
        {
            Ok(in_flight_request)
        } else if access_token.has_permission(Permission::UnlimitedRequests) {
            Ok(InFlight::default())
        } else {
            Err(trc::LimitEvent::ConcurrentUpload.into_err())
        }
    }
}
