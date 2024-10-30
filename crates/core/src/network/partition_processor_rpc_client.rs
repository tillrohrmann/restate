// Copyright (c) 2024 - Restate Software, Inc., Restate GmbH.
// All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

use assert2::let_assert;
use restate_types::identifiers::{InvocationId, PartitionId, PartitionProcessorRpcRequestId, WithPartitionKey};
use restate_types::invocation::{InvocationQuery, InvocationResponse, ServiceInvocation};
use restate_types::live::Live;
use restate_types::net::partition_processor::{
    AppendInvocationReplyOn, GetInvocationOutputResponseMode, InvocationOutput,
    PartitionProcessorRpcError, PartitionProcessorRpcRequest, PartitionProcessorRpcRequestInner,
    PartitionProcessorRpcResponse, StreamingPartitionProcessorRequestKind,
    StreamingPartitionProcessorRpcRequest, StreamingPartitionProcessorRpcResponse,
    SubmittedInvocationNotification,
};
use restate_types::partition_table::{FindPartition, PartitionTable, PartitionTableError};

use crate::network::rpc_router::{ConnectionStream, RpcError, RpcRouter, StreamingRpcRouter};
use crate::network::{Networking, TransportConnect};
use crate::routing_info::PartitionRouting;
use crate::ShutdownError;

#[derive(Debug, thiserror::Error)]
pub enum PartitionProcessorRpcClientError {
    #[error(transparent)]
    UnknownPartition(#[from] PartitionTableError),
    #[error("cannot find node per partition {0}")]
    UnknownNodePerPartition(PartitionId),
    #[error("failed sending request")]
    SendFailed,
    #[error(transparent)]
    Shutdown(#[from] ShutdownError),
    #[error("operation failed: {0}")]
    Internal(String),
}

impl<T> From<RpcError<T>> for PartitionProcessorRpcClientError {
    fn from(value: RpcError<T>) -> Self {
        match value {
            RpcError::SendError(_) => PartitionProcessorRpcClientError::SendFailed,
            RpcError::Shutdown(err) => PartitionProcessorRpcClientError::Shutdown(err),
        }
    }
}

#[derive(Debug, Clone)]
pub enum AttachInvocationResponse {
    NotFound,
    NotSupported,
    Ready(InvocationOutput),
}

#[derive(Debug, Clone)]
pub enum GetInvocationOutputResponse {
    NotFound,
    NotReady,
    NotSupported,
    Ready(InvocationOutput),
}

pub struct PartitionProcessorRpcClient<T> {
    networking: Networking<T>,
    rpc_router: RpcRouter<PartitionProcessorRpcRequest>,
    streaming_rpc_router: StreamingRpcRouter<StreamingPartitionProcessorRpcRequest>,
    partition_table: Live<PartitionTable>,
    partition_routing: PartitionRouting,
}

impl<T> Clone for PartitionProcessorRpcClient<T> {
    fn clone(&self) -> Self {
        Self {
            networking: self.networking.clone(),
            rpc_router: self.rpc_router.clone(),
            streaming_rpc_router: self.streaming_rpc_router.clone(),
            partition_table: self.partition_table.clone(),
            partition_routing: self.partition_routing.clone(),
        }
    }
}

impl<T> PartitionProcessorRpcClient<T> {
    pub fn new(
        networking: Networking<T>,
        rpc_router: RpcRouter<PartitionProcessorRpcRequest>,
        streaming_rpc_router: StreamingRpcRouter<StreamingPartitionProcessorRpcRequest>,
        partition_table: Live<PartitionTable>,
        partition_routing: PartitionRouting,
    ) -> Self {
        Self {
            networking,
            rpc_router,
            streaming_rpc_router,
            partition_table,
            partition_routing,
        }
    }
}

impl<T> PartitionProcessorRpcClient<T>
where
    T: TransportConnect,
{

    pub async fn request_refresh(&self) {
        self.partition_routing.request_refresh().await;
    }

    pub async fn submit_invocation(
        &self,
        request_id: PartitionProcessorRpcRequestId,
        service_invocation: ServiceInvocation,
    ) -> Result<
        ConnectionStream<Result<StreamingPartitionProcessorRpcResponse, PartitionProcessorRpcError>>,
        PartitionProcessorRpcClientError,
    > {
        let partition_id = self
            .partition_table
            .pinned()
            .find_partition_id(service_invocation.invocation_id.partition_key())?;

        let node_id = self
            .partition_routing
            .get_node_by_partition(partition_id)
            .ok_or(PartitionProcessorRpcClientError::UnknownNodePerPartition(
                partition_id,
            ))?;

        let response_stream = self
            .streaming_rpc_router
            .call(
                &self.networking,
                node_id,
                StreamingPartitionProcessorRpcRequest {
                    request_id,
                    partition_id,
                    request: StreamingPartitionProcessorRequestKind::Invoke(service_invocation),
                },
            )
            .await?;

        Ok(response_stream)
    }

    pub async fn attach_to_invocation(
        &self,
        request_id: PartitionProcessorRpcRequestId,
        invocation_id: InvocationId,
    ) -> Result<
        ConnectionStream<Result<StreamingPartitionProcessorRpcResponse, PartitionProcessorRpcError>>,
        PartitionProcessorRpcClientError,
    > {
        let partition_id = self
            .partition_table
            .pinned()
            .find_partition_id(invocation_id.partition_key())?;

        let node_id = self
            .partition_routing
            .get_node_by_partition(partition_id)
            .ok_or(PartitionProcessorRpcClientError::UnknownNodePerPartition(
                partition_id,
            ))?;

        let response_stream = self
            .streaming_rpc_router
            .call(
                &self.networking,
                node_id,
                StreamingPartitionProcessorRpcRequest {
                    request_id,
                    partition_id,
                    request: StreamingPartitionProcessorRequestKind::Attach(InvocationQuery::Invocation(invocation_id)),
                },
            )
            .await?;

        Ok(response_stream)
    }

    pub async fn append_invocation(
        &self,
        request_id: PartitionProcessorRpcRequestId,
        service_invocation: ServiceInvocation,
    ) -> Result<(), PartitionProcessorRpcClientError> {
        let response = self
            .resolve_partition_id_and_send(
                request_id,
                PartitionProcessorRpcRequestInner::AppendInvocation(
                    service_invocation,
                    AppendInvocationReplyOn::Appended,
                ),
            )
            .await?;

        let_assert!(
            PartitionProcessorRpcResponse::Appended = response,
            "Expecting PartitionProcessorRpcResponse::Appended"
        );

        Ok(())
    }

    pub async fn append_invocation_and_wait_submit_notification(
        &self,
        request_id: PartitionProcessorRpcRequestId,
        service_invocation: ServiceInvocation,
    ) -> Result<SubmittedInvocationNotification, PartitionProcessorRpcClientError> {
        let response = self
            .resolve_partition_id_and_send(
                request_id,
                PartitionProcessorRpcRequestInner::AppendInvocation(
                    service_invocation,
                    AppendInvocationReplyOn::Submitted,
                ),
            )
            .await?;

        let_assert!(
            PartitionProcessorRpcResponse::Submitted(submit_notification) = response,
            "Expecting PartitionProcessorRpcResponse::Submitted"
        );
        debug_assert_eq!(
            request_id, submit_notification.request_id,
            "Conflicting submit notification received"
        );

        Ok(submit_notification)
    }

    pub async fn append_invocation_and_wait_output(
        &self,
        request_id: PartitionProcessorRpcRequestId,
        service_invocation: ServiceInvocation,
    ) -> Result<InvocationOutput, PartitionProcessorRpcClientError> {
        let response = self
            .resolve_partition_id_and_send(
                request_id,
                PartitionProcessorRpcRequestInner::AppendInvocation(
                    service_invocation,
                    AppendInvocationReplyOn::Output,
                ),
            )
            .await?;

        let_assert!(
            PartitionProcessorRpcResponse::Output(invocation_output) = response,
            "Expecting PartitionProcessorRpcResponse::Output"
        );
        debug_assert_eq!(
            request_id, invocation_output.request_id,
            "Conflicting invocation output received"
        );

        Ok(invocation_output)
    }

    pub async fn attach_invocation(
        &self,
        request_id: PartitionProcessorRpcRequestId,
        invocation_query: InvocationQuery,
    ) -> Result<AttachInvocationResponse, PartitionProcessorRpcClientError> {
        let response = self
            .resolve_partition_id_and_send(
                request_id,
                PartitionProcessorRpcRequestInner::GetInvocationOutput(
                    invocation_query,
                    GetInvocationOutputResponseMode::BlockWhenNotReady,
                ),
            )
            .await?;

        Ok(match response {
            PartitionProcessorRpcResponse::NotFound => AttachInvocationResponse::NotFound,
            PartitionProcessorRpcResponse::NotSupported => AttachInvocationResponse::NotSupported,
            PartitionProcessorRpcResponse::Output(output) => {
                AttachInvocationResponse::Ready(output)
            }
            _ => {
                panic!("Expecting either PartitionProcessorRpcResponse::Output or PartitionProcessorRpcResponse::NotFound or PartitionProcessorRpcResponse::NotSupported")
            }
        })
    }

    pub async fn get_invocation_output(
        &self,
        request_id: PartitionProcessorRpcRequestId,
        invocation_query: InvocationQuery,
    ) -> Result<GetInvocationOutputResponse, PartitionProcessorRpcClientError> {
        let response = self
            .resolve_partition_id_and_send(
                request_id,
                PartitionProcessorRpcRequestInner::GetInvocationOutput(
                    invocation_query,
                    GetInvocationOutputResponseMode::ReplyIfNotReady,
                ),
            )
            .await?;

        Ok(match response {
            PartitionProcessorRpcResponse::NotFound => GetInvocationOutputResponse::NotFound,
            PartitionProcessorRpcResponse::NotSupported => {
                GetInvocationOutputResponse::NotSupported
            }
            PartitionProcessorRpcResponse::NotReady => GetInvocationOutputResponse::NotReady,
            PartitionProcessorRpcResponse::Output(output) => {
                GetInvocationOutputResponse::Ready(output)
            }
            _ => {
                panic!("Expecting either PartitionProcessorRpcResponse::Output or PartitionProcessorRpcResponse::NotFound or PartitionProcessorRpcResponse::NotSupported or PartitionProcessorRpcResponse::NotReady")
            }
        })
    }

    pub async fn append_invocation_response(
        &self,
        request_id: PartitionProcessorRpcRequestId,
        invocation_response: InvocationResponse,
    ) -> Result<(), PartitionProcessorRpcClientError> {
        let response = self
            .resolve_partition_id_and_send(
                request_id,
                PartitionProcessorRpcRequestInner::AppendInvocationResponse(invocation_response),
            )
            .await?;

        let_assert!(
            PartitionProcessorRpcResponse::Appended = response,
            "Expecting PartitionProcessorRpcResponse::Appended"
        );

        Ok(())
    }

    async fn resolve_partition_id_and_send(
        &self,
        request_id: PartitionProcessorRpcRequestId,
        inner_request: PartitionProcessorRpcRequestInner,
    ) -> Result<PartitionProcessorRpcResponse, PartitionProcessorRpcClientError> {
        let partition_id = self
            .partition_table
            .pinned()
            .find_partition_id(inner_request.partition_key())?;

        // TODO Find node on which the leader for the given partition runs
        let node_id = self
            .partition_routing
            .get_node_by_partition(partition_id)
            .ok_or(PartitionProcessorRpcClientError::UnknownNodePerPartition(
                partition_id,
            ))?;
        let response = self
            .rpc_router
            .call(
                &self.networking,
                node_id,
                PartitionProcessorRpcRequest {
                    request_id,
                    partition_id,
                    inner: inner_request,
                },
            )
            .await?;

        response
            .into_body()
            .map_err(|err| PartitionProcessorRpcClientError::Internal(err.to_string()))
    }
}
