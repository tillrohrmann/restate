// Copyright (c) 2024 -  Restate Software, Inc., Restate GmbH.
// All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

use anyhow::{anyhow, Context};
use futures::{StreamExt};
use restate_core::network::partition_processor_rpc_client::{
    AttachInvocationResponse, GetInvocationOutputResponse,
};
use restate_core::network::partition_processor_rpc_client::{
    PartitionProcessorRpcClient, PartitionProcessorRpcClientError,
};
use restate_core::network::TransportConnect;
use restate_ingress_http::{RequestDispatcher, RequestDispatcherError};
use restate_types::identifiers::PartitionProcessorRpcRequestId;
use restate_types::invocation::{
    InvocationQuery, InvocationResponse, InvocationTargetType, ServiceInvocation,
    WorkflowHandlerType,
};
use restate_types::net::partition_processor::{
    InvocationOutput, PartitionProcessorRpcError,
    StreamingPartitionProcessorRpcResponse,
    SubmittedInvocationNotification,
};
use restate_types::retries::RetryPolicy;
use std::time::Duration;
use tracing::debug;

pub struct RpcRequestDispatcher<T> {
    partition_processor_rpc_client: PartitionProcessorRpcClient<T>,

    retry_policy: RetryPolicy,
    rpc_timeout: Duration,
}

impl<T> Clone for RpcRequestDispatcher<T> {
    fn clone(&self) -> Self {
        Self {
            partition_processor_rpc_client: self.partition_processor_rpc_client.clone(),
            retry_policy: self.retry_policy.clone(),
            rpc_timeout: self.rpc_timeout,
        }
    }
}


impl<T> RpcRequestDispatcher<T>
where
    T: TransportConnect,
{
    pub fn new(
        partition_processor_rpc_client: PartitionProcessorRpcClient<T>,
    ) -> Self {
        Self {
            partition_processor_rpc_client,
            // Totally random chosen!!!
            retry_policy: RetryPolicy::fixed_delay(Duration::from_millis(50), None),
            rpc_timeout: Duration::from_secs(5),
        }
    }
}

impl<T> RequestDispatcher for RpcRequestDispatcher<T>
where
    T: TransportConnect,
{
    async fn submit_invocation(
        &self,
        service_invocation: ServiceInvocation,
    ) -> Result<InvocationOutput, RequestDispatcherError> {
        let request_id = PartitionProcessorRpcRequestId::default();
        let is_idempotent = service_invocation.is_idempotent();
        let invocation_id = service_invocation.invocation_id;

        let mut retry_iter = RetryPolicy::exponential(
            Duration::from_millis(100),
            2.0,
            Some(20),
            Some(Duration::from_secs(5)),
        )
        .into_iter();

        // submit the invocation
        let response_stream = loop {
            let mut response_stream = match self
                .partition_processor_rpc_client
                .submit_invocation(request_id, service_invocation.clone())
                .await
            {
                Ok(response_stream) => response_stream,
                Err(rpc_error) => {
                    debug!("failed sending submit invocation request to partition processor: {rpc_error}");

                    match rpc_error {
                        PartitionProcessorRpcClientError::Shutdown(_) => return Err(anyhow!("shutdown").into()),
                        err => {
                            if let Some(retry_delay) = retry_iter.next() {
                                // try again if we have attempts left
                                tokio::time::sleep(retry_delay).await;
                                continue;
                            } else {
                                return Err(anyhow!(
                                    "failed submitting invocation: {}",
                                    err
                                )
                                    .into());
                            }
                        }
                    }
                }
            };

            match response_stream.next().await {
                None => {
                    if !is_idempotent {
                        // stream was closed unexpectedly, we don't know whether the submission happened
                        // or not. Since we don't want to submit invocations multiple times, we have to
                        // assume that it was successful
                        debug!("Failing non-idempotent service invocation submission because the response stream was closed unexpectedly.");
                        return Err(anyhow!("stream was closed unexpectedly").into());
                    }
                }
                Some(Err(err)) => {
                    if !is_idempotent {
                        // encountered a network error, we don't know whether the submission happened
                        // or not. Since we don't want to submit invocations multiple times, we have to
                        // assume that it was successful
                        debug!("Failing non-idempotent service invocation submission because of network error: {err}.");
                        return Err(anyhow!("network error: {err}").into());
                    }
                }
                Some(Ok(response)) => {
                    match response.into_body() {
                        Ok(StreamingPartitionProcessorRpcResponse::Submitted(
                            submit_notification,
                        )) => {
                            debug!("Received submit notification: {submit_notification:?}");
                            // continue now with the receiving the output
                            break response_stream;
                        }
                        Ok(response) => {
                            // receiving an unexpected response is a bug
                            panic!("Received unexpected response while awaiting submit notification: {response:?}");
                        }
                        Err(err) => {
                            match &err {
                                PartitionProcessorRpcError::NotLeader(_) => {
                                    self.partition_processor_rpc_client.request_refresh().await;
                                    // todo continue right after we have received an update
                                }
                                PartitionProcessorRpcError::Busy => {
                                    // let's retry if we have attempts left
                                    // todo figure out whether to wait longer to avoid overloading the system
                                }
                                PartitionProcessorRpcError::Internal(err) => {
                                    // it's unclear whether the submission was successful or not,
                                    // let's be conservative and fail if our invocation is not idempotent
                                    if !is_idempotent {
                                        return Err(anyhow!("internal error: {err}").into());
                                    }
                                }
                            }

                            if let Some(retry_delay) = retry_iter.next() {
                                // try again if we have attempts left
                                tokio::time::sleep(retry_delay).await;
                                continue;
                            } else {
                                return Err(anyhow!("rpc error: {err}").into());
                            }
                        }
                    }
                }
            }
        };

        let mut response_stream_holder = Some(response_stream);

        // get the output
        loop {
            let mut response_stream = match response_stream_holder.take() {
                None => {
                    // previous response stream closed, try again opening it
                    match self
                        .partition_processor_rpc_client
                        .attach_to_invocation(request_id, invocation_id)
                        .await
                    {
                        Ok(response_stream) => response_stream,
                        Err(err) => {
                            debug!("failed sending attach invocation request to partition processor: {err}");

                            match err {
                                PartitionProcessorRpcClientError::Shutdown(_) => return Err(anyhow!("shutdown").into()),
                                err => {
                                    if let Some(retry_delay) = retry_iter.next() {
                                        // try again if we have attempts left
                                        tokio::time::sleep(retry_delay).await;
                                        continue;
                                    } else {
                                        return Err(anyhow!(
                                    "failed attaching to invocation: {}",
                                    err
                                )
                                            .into());
                                    }
                                }
                            }
                        }
                    }
                }
                Some(response_stream) => response_stream,
            };

            let err = match response_stream.next().await {
                None => {
                    anyhow!("Response stream was closed unexpectedly.")
                }
                Some(Err(err)) => {
                    anyhow!("network error: {err}")
                }
                Some(Ok(response)) => {
                    match response.into_body() {
                        Ok(StreamingPartitionProcessorRpcResponse::Output(output)) => {
                            return Ok(output);
                        }
                        Ok(response) => {
                            // receiving an unexpected response is a bug
                            panic!("Received unexpected response while awaiting invocation output: {response:?}");
                        }
                        Err(err) => {
                            match &err {
                                PartitionProcessorRpcError::NotLeader(_) => {
                                    self.partition_processor_rpc_client.request_refresh().await;
                                    // todo continue right after we have received an update
                                }
                                PartitionProcessorRpcError::Busy => {
                                    // let's retry if we have attempts left
                                    // todo figure out whether to wait longer to avoid overloading the system
                                }
                                PartitionProcessorRpcError::Internal(_) => {}
                            }

                            anyhow!("rpc error: {err}")
                        }
                    }
                }
            };

            if let Some(retry_delay) = retry_iter.next() {
                debug!("Retrying attaching to invocation because of error: {err}");
                // try again if we have attempts left
                tokio::time::sleep(retry_delay).await;
            } else {
                return Err(err.into());
            }
        }
    }

    async fn append_invocation_and_wait_submit_notification_if_needed(
        &self,
        service_invocation: ServiceInvocation,
    ) -> Result<SubmittedInvocationNotification, RequestDispatcherError> {
        let request_id = PartitionProcessorRpcRequestId::default();
        if service_invocation.idempotency_key.is_some()
            || service_invocation.invocation_target.invocation_target_ty()
                == InvocationTargetType::Workflow(WorkflowHandlerType::Workflow)
        {
            // In this case we need to wait for the submit notification from the PP, this is safe to retry
            self.retry_policy
                .clone()
                .retry(|| async {
                    Ok(tokio::time::timeout(
                        self.rpc_timeout,
                        self.partition_processor_rpc_client
                            .append_invocation_and_wait_submit_notification(
                                request_id,
                                service_invocation.clone(),
                            ),
                    )
                    .await
                    .context("timeout while trying to reach partition processor")?
                    .context("error when trying to interact with partition processor")?)
                })
                .await
        } else {
            // In this case we just need to wait the invocation was appended
            tokio::time::timeout(
                self.rpc_timeout,
                self.partition_processor_rpc_client
                    .append_invocation(request_id, service_invocation),
            )
            .await
            .context("timeout while trying to reach partition processor")?
            .context("error when trying to interact with partition processor")?;
            Ok(SubmittedInvocationNotification {
                request_id,
                is_new_invocation: true,
            })
        }
    }

    async fn append_invocation_and_wait_output(
        &self,
        service_invocation: ServiceInvocation,
    ) -> Result<InvocationOutput, RequestDispatcherError> {
        let request_id = PartitionProcessorRpcRequestId::default();
        if service_invocation.idempotency_key.is_some()
            || service_invocation.invocation_target.invocation_target_ty()
                == InvocationTargetType::Workflow(WorkflowHandlerType::Workflow)
        {
            // For workflow or idempotent calls, it is safe to retry always
            self.retry_policy
                .clone()
                .retry(|| async {
                    Ok(tokio::time::timeout(
                        self.rpc_timeout,
                        self.partition_processor_rpc_client
                            .append_invocation_and_wait_output(
                                request_id,
                                service_invocation.clone(),
                            ),
                    )
                    .await
                    .context("timeout while trying to reach partition processor")?
                    .context("error when trying to interact with partition processor")?)
                })
                .await
        } else {
            Ok(tokio::time::timeout(
                self.rpc_timeout,
                self.partition_processor_rpc_client
                    .append_invocation_and_wait_output(request_id, service_invocation),
            )
            .await
            .context("timeout while trying to reach partition processor")?
            .context("error when trying to interact with partition processor")?)
        }
    }

    async fn attach_invocation(
        &self,
        invocation_query: InvocationQuery,
    ) -> Result<AttachInvocationResponse, RequestDispatcherError> {
        let request_id = PartitionProcessorRpcRequestId::default();
        // Attaching an invocation is idempotent and can be retried, with timeouts
        self.retry_policy
            .clone()
            .retry(|| async {
                Ok(tokio::time::timeout(
                    self.rpc_timeout,
                    self.partition_processor_rpc_client
                        .attach_invocation(request_id, invocation_query.clone()),
                )
                .await
                .context("timeout while trying to reach partition processor")?
                .context("error when trying to interact with partition processor")?)
            })
            .await
    }

    async fn get_invocation_output(
        &self,
        invocation_query: InvocationQuery,
    ) -> Result<GetInvocationOutputResponse, RequestDispatcherError> {
        // No need to retry this
        Ok(tokio::time::timeout(
            self.rpc_timeout,
            self.partition_processor_rpc_client
                .get_invocation_output(PartitionProcessorRpcRequestId::default(), invocation_query),
        )
        .await
        .context("timeout while trying to reach partition processor")?
        .context("error when trying to interact with partition processor")?)
    }

    async fn append_invocation_response(
        &self,
        invocation_response: InvocationResponse,
    ) -> Result<(), RequestDispatcherError> {
        let request_id = PartitionProcessorRpcRequestId::default();
        // Appending invocation response is an idempotent operation, it's always safe to try
        self.retry_policy
            .clone()
            .retry(|| async {
                Ok(self
                    .partition_processor_rpc_client
                    .append_invocation_response(request_id, invocation_response.clone())
                    .await
                    .context("error when trying to interact with partition processor")?)
            })
            .await
    }
}
