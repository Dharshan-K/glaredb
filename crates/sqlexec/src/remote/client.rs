use crate::{
    errors::{ExecError, Result},
    extension_codec::GlareDBExtensionCodec,
    metastore::catalog::SessionCatalog,
};
use datafusion::{datasource::TableProvider, physical_plan::ExecutionPlan};
use datafusion_ext::functions::FuncParamValue;
use datafusion_proto::{physical_plan::AsExecutionPlan, protobuf::PhysicalPlanNode};
use protogen::{
    gen::rpcsrv::common,
    gen::rpcsrv::service::{self, execution_service_client::ExecutionServiceClient},
    metastore::types::catalog::CatalogState,
    rpcsrv::types::service::{
        DispatchAccessRequest, FetchCatalogRequest, FetchCatalogResponse, InitializeSessionRequest,
        InitializeSessionResponse, PhysicalPlanExecuteRequest, ResolvedTableReference,
        TableProviderResponse,
    },
};
use proxyutil::metadata_constants::{
    COMPUTE_ENGINE_KEY, DB_NAME_KEY, ORG_KEY, PASSWORD_KEY, USER_KEY,
};
use serde::Deserialize;
use std::{collections::HashMap, sync::Arc};
use tonic::{
    metadata::MetadataMap,
    transport::{Certificate, Channel, ClientTlsConfig, Endpoint},
    IntoRequest, Streaming,
};
use tracing::info;
use url::Url;
use uuid::Uuid;

use super::table::StubRemoteTableProvider;

const DEFAULT_RPC_PROXY_PORT: u16 = 6443;

/// Params that need to be set on grpc connections when going through the proxy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyAuthParams {
    /// User (generated Cloud credentials)
    pub user: String,
    /// Password (generated Cloud credentials)
    pub password: String,
    /// DB name
    pub db_name: String,
    /// Org name.
    pub org: String,
    /// Compute engine name.
    pub compute_engine: Option<String>,
}

/// Auth params and destination to use when connecting the client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyDestination {
    pub params: ProxyAuthParams,
    pub dst: Url,
}

impl TryFrom<Url> for ProxyDestination {
    type Error = ExecError;

    /// Try to parse authentication parameters and destinaton from a url.
    fn try_from(value: Url) -> Result<Self, Self::Error> {
        if value.scheme() != "glaredb" {
            return Err(ExecError::InvalidRemoteExecUrl(
                "URL must start with 'glaredb://'".to_string(),
            ));
        }

        let user = value.username();
        let password = value
            .password()
            .ok_or_else(|| ExecError::InvalidRemoteExecUrl("Missing password".to_string()))?;

        let host = value
            .host_str()
            .ok_or_else(|| ExecError::InvalidRemoteExecUrl("URL is missing a host".to_string()))?;

        // Host should be in the form "orgname.remote.glaredb.com"
        let (org, host) = host
            .split_once('.')
            .ok_or_else(|| ExecError::InvalidRemoteExecUrl("Invalid host".to_string()))?;

        // Remove leading slash from path, use that as database name.
        let db_name = value.path().trim_start_matches('/');
        if db_name.is_empty() {
            return Err(ExecError::InvalidRemoteExecUrl(
                "Missing db name".to_string(),
            ));
        }

        // Database name could be just the name itself, or may be in the form of
        // "engine.dbname".
        let (compute_engine, db_name) =
            if let Some((compute_engine, db_name)) = db_name.split_once('.') {
                (Some(compute_engine), db_name)
            } else {
                (None, db_name)
            };

        // Rebuild url that we should actually connect to.
        let dst = Url::parse(&format!(
            "http://{host}:{}",
            value.port().unwrap_or(DEFAULT_RPC_PROXY_PORT)
        ))
        .map_err(|e| {
            ExecError::Internal(format!("fail to parse reconstructed host and port: {e}"))
        })?;

        let params = ProxyAuthParams {
            user: user.to_string(),
            password: password.to_string(),
            db_name: db_name.to_string(),
            org: org.to_string(),
            compute_engine: compute_engine.map(String::from),
        };

        Ok(ProxyDestination { params, dst })
    }
}

#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct AuthenticateClientResponse {
    pub ca_cert: String,
    pub ca_domain: String,
}

/// An execution service client that has additonal metadata attached to each
/// request for authentication through the proxy.
#[derive(Debug, Clone)]
pub struct RemoteClient {
    /// The inner client.
    client: ExecutionServiceClient<Channel>,

    /// The auth metadata that gets placed on all requests.
    auth_metadata: Arc<MetadataMap>,
}

impl RemoteClient {
    /// Connect to destination without any additional authentication metadata.
    ///
    /// This can be used for testing (and possibly for inter-node
    /// communication?).
    pub async fn connect(dst: Url) -> Result<Self> {
        let client = ExecutionServiceClient::connect(dst.to_string()).await?;
        Ok(RemoteClient {
            client,
            auth_metadata: Arc::new(MetadataMap::new()),
        })
    }

    /// Get the deployment name that we're connected to from the stored metadata
    /// map.
    pub fn get_deployment_name(&self) -> &str {
        self.auth_metadata
            .get(DB_NAME_KEY)
            .map(|m| m.to_str().unwrap_or_default())
            .unwrap_or("unknown")
    }

    /// Connect to a proxy destination.
    pub async fn connect_with_proxy_destination(
        dst: ProxyDestination,
        cloud_api_addr: String,
        disable_tls: bool,
    ) -> Result<Self> {
        let mut dst: ProxyDestination = dst;
        if !disable_tls {
            info!("set rpc destination scheme to https");

            dst.dst
                .set_scheme("https")
                .expect("failed to upgrade scheme from http to https");
        }
        Self::connect_with_proxy_auth_params(
            dst.dst.to_string(),
            dst.params,
            cloud_api_addr,
            disable_tls,
        )
        .await
    }

    /// Connect to a destination with the provided authentication params.
    async fn connect_with_proxy_auth_params<'a>(
        dst: impl TryInto<Endpoint, Error = tonic::transport::Error>,
        params: ProxyAuthParams,
        cloud_api_addr: String,
        disable_tls: bool,
    ) -> Result<Self> {
        let mut metadata = MetadataMap::new();
        metadata.insert(USER_KEY, params.user.parse()?);
        metadata.insert(PASSWORD_KEY, params.password.parse()?);
        metadata.insert(DB_NAME_KEY, params.db_name.parse()?);
        metadata.insert(ORG_KEY, params.org.parse()?);
        if let Some(compute_engine) = params.compute_engine {
            metadata.insert(COMPUTE_ENGINE_KEY, compute_engine.parse()?);
        }

        let mut dst: Endpoint = dst.try_into()?;

        if !disable_tls {
            info!("apply TLS certificate to rpc proxy connection");

            let mut body = HashMap::new();
            body.insert("user", params.user);
            body.insert("password", params.password);
            body.insert("org_name", params.org);
            body.insert("db_name", params.db_name);

            let client = reqwest::Client::new();
            let res = client
                .post(format!(
                    "{}/api/internal/authenticate/client",
                    cloud_api_addr
                ))
                .json(&body)
                .send()
                .await?
                .json::<AuthenticateClientResponse>()
                .await?;

            dst = dst.tls_config(
                ClientTlsConfig::new()
                    .ca_certificate(Certificate::from_pem(res.ca_cert))
                    .domain_name(res.ca_domain),
            )?;
        }

        let client = ExecutionServiceClient::connect(dst).await?;

        Ok(RemoteClient {
            client,
            auth_metadata: Arc::new(metadata),
        })
    }

    pub async fn initialize_session(
        &mut self,
        request: InitializeSessionRequest,
    ) -> Result<(RemoteSessionClient, SessionCatalog)> {
        let mut request = service::InitializeSessionRequest::from(request).into_request();
        self.append_auth_metadata(request.metadata_mut());

        let resp = self.client.initialize_session(request).await.map_err(|e| {
            ExecError::RemoteSession(format!("failed to initialize remote session: {e}"))
        })?;
        let resp: InitializeSessionResponse = resp.into_inner().try_into()?;

        let remote_sess_client = RemoteSessionClient {
            inner: self.clone(),
            database_id: resp.database_id,
        };

        Ok((
            remote_sess_client,
            SessionCatalog::new(Arc::new(resp.catalog)),
        ))
    }

    fn append_auth_metadata(&self, metadata: &mut MetadataMap) {
        for kv in self.auth_metadata.iter() {
            match kv {
                tonic::metadata::KeyAndValueRef::Ascii(key, val) => {
                    metadata.append(key, val.clone())
                }
                tonic::metadata::KeyAndValueRef::Binary(key, val) => {
                    metadata.append_bin(key, val.clone())
                }
            };
        }
    }
}

/// A client to interact with the current active remote session.
#[derive(Debug, Clone)]
pub struct RemoteSessionClient {
    inner: RemoteClient,
    database_id: Uuid,
}

impl RemoteSessionClient {
    /// Returns the database ID for which the session is open.
    pub fn database_id(&self) -> Uuid {
        self.database_id
    }

    pub fn get_deployment_name(&self) -> &str {
        self.inner.get_deployment_name()
    }

    pub async fn fetch_catalog(&mut self) -> Result<CatalogState> {
        let mut request = service::FetchCatalogRequest::from(FetchCatalogRequest {
            database_id: self.database_id(),
        })
        .into_request();
        self.inner.append_auth_metadata(request.metadata_mut());

        let resp: FetchCatalogResponse = self
            .inner
            .client
            .fetch_catalog(request)
            .await
            .map_err(|e| ExecError::RemoteSession(format!("failed to fetch catalog: {e}")))?
            .into_inner()
            .try_into()?;

        Ok(resp.catalog)
    }

    pub async fn dispatch_access(
        &mut self,
        table_ref: ResolvedTableReference,
        args: Option<Vec<FuncParamValue>>,
        opts: Option<HashMap<String, FuncParamValue>>,
    ) -> Result<Arc<dyn TableProvider>> {
        let args = args
            .map(|arg| {
                arg.into_iter()
                    .map(|arg| Ok(arg.try_into()?))
                    .collect::<Result<Vec<_>>>()
            })
            .transpose()?;

        let opts = opts
            .map(|opts| {
                opts.into_iter()
                    .map(|(k, v)| Ok((k, v.try_into()?)))
                    .collect::<Result<HashMap<_, _>>>()
            })
            .transpose()?;
        let mut request = service::DispatchAccessRequest::from(DispatchAccessRequest {
            database_id: self.database_id(),
            table_ref,
            args,
            opts,
        })
        .into_request();
        self.inner.append_auth_metadata(request.metadata_mut());

        let resp: TableProviderResponse = self
            .inner
            .client
            .dispatch_access(request)
            .await
            .map_err(|e| ExecError::RemoteSession(format!("unable to dispatch table access: {e}")))?
            .into_inner()
            .try_into()?;

        Ok(Arc::new(StubRemoteTableProvider::new(resp.id, Arc::new(resp.schema))) as _)
    }

    pub async fn physical_plan_execute(
        &mut self,
        physical_plan: Arc<dyn ExecutionPlan>,
    ) -> Result<Streaming<service::RecordBatchResponse>> {
        // Encode the physical plan into a protobuf message.
        let physical_plan = {
            let node = PhysicalPlanNode::try_from_physical_plan(
                physical_plan,
                &GlareDBExtensionCodec::new_encoder(),
            )?;
            let mut buf = Vec::new();
            node.try_encode(&mut buf)?;
            buf
        };

        let mut request = service::PhysicalPlanExecuteRequest::from(PhysicalPlanExecuteRequest {
            database_id: self.database_id(),
            physical_plan,
        })
        .into_request();
        self.inner.append_auth_metadata(request.metadata_mut());

        let resp = self
            .inner
            .client
            .physical_plan_execute(request)
            .await
            .map_err(|e| {
                ExecError::RemoteSession(format!("error while executing physical plan: {e}"))
            })?
            .into_inner();
        Ok(resp)
    }

    pub async fn broadcast_exchange(
        &mut self,
        stream: impl tonic::IntoStreamingRequest<Message = common::ExecutionResultBatch>,
    ) -> Result<()> {
        let mut req = stream.into_streaming_request();
        self.inner.append_auth_metadata(req.metadata_mut());
        let _resp = self.inner.client.broadcast_exchange(req).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params_from_url_valid_default_port() {
        let out = ProxyDestination::try_from(
            Url::parse("glaredb://user:password@org.remote.glaredb.com/db").unwrap(),
        )
        .unwrap();

        let expected = ProxyDestination {
            params: ProxyAuthParams {
                user: "user".to_string(),
                password: "password".to_string(),
                db_name: "db".to_string(),
                org: "org".to_string(),
                compute_engine: None,
            },
            dst: Url::parse("http://remote.glaredb.com:6443").unwrap(),
        };

        assert_eq!(expected, out);
    }

    #[test]
    fn params_from_url_valid_port_and_engine() {
        let out = ProxyDestination::try_from(
            Url::parse("glaredb://user:password@org.remote.glaredb.com:4444/engine.db").unwrap(),
        )
        .unwrap();

        let expected = ProxyDestination {
            params: ProxyAuthParams {
                user: "user".to_string(),
                password: "password".to_string(),
                db_name: "db".to_string(),
                org: "org".to_string(),
                compute_engine: Some("engine".to_string()),
            },
            dst: Url::parse("http://remote.glaredb.com:4444").unwrap(),
        };

        assert_eq!(expected, out);
    }

    #[test]
    fn params_from_url_invalid() {
        // Invalid scheme
        ProxyDestination::try_from(
            Url::parse("http://user:password@org.remote.glaredb.com:4444/engine.db").unwrap(),
        )
        .unwrap_err();

        // Missing password
        ProxyDestination::try_from(
            Url::parse("glaredb://user@org.remote.glaredb.com:4444/engine.db").unwrap(),
        )
        .unwrap_err();

        // Missing db name
        ProxyDestination::try_from(
            Url::parse("glaredb://user:password@org.remote.glaredb.com:4444/").unwrap(),
        )
        .unwrap_err();
    }
}
