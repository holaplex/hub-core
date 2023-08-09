use std::error::Error;

pub use hub_core_macros::Triage;

/// Information about the severity of an error
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Severity {
    /// The error is recoverable and a retry should be attempted
    Transient,
    /// The error is unrecoverable, the current operation should not be retried
    Permanent,
    /// The error is unrecoverable and the service should terminate immediately
    Fatal,
}

/// Type alias for a boxed [`dyn Triage`](Triage)
pub type Boxed<'a> = Box<dyn Triage + 'a>;
/// Type alias for a boxed [`dyn Triage`](Triage) with [`Send`] and [`Sync`]
pub type BoxedSync<'a> = Box<dyn Triage + Send + Sync + 'a>;

/// An error containing information about its severity
pub trait Triage: Error {
    /// Get the severity level of this error
    fn severity(&self) -> Severity;
}

impl Triage for bs58::decode::Error {
    #[inline]
    fn severity(&self) -> Severity {
        Severity::Permanent
    }
}

impl Triage for std::io::Error {
    #[inline]
    fn severity(&self) -> Severity {
        use std::io::ErrorKind;

        match self.kind() {
            ErrorKind::NotFound
            | ErrorKind::ConnectionRefused
            | ErrorKind::ConnectionReset
            | ErrorKind::ConnectionAborted
            | ErrorKind::NotConnected
            | ErrorKind::AddrInUse
            | ErrorKind::BrokenPipe
            | ErrorKind::WouldBlock
            | ErrorKind::TimedOut
            | ErrorKind::WriteZero
            | ErrorKind::Interrupted => Severity::Transient,
            _ => Severity::Permanent,
        }
    }
}

#[cfg(feature = "jsonrpsee-core")]
impl Triage for jsonrpsee_core::Error {
    #[inline]
    fn severity(&self) -> Severity {
        use jsonrpsee_core::Error;

        match self {
            Error::Call(_)
            | Error::Transport(_)
            | Error::InvalidResponse(_)
            | Error::RestartNeeded(_)
            | Error::ParseError(_)
            | Error::InvalidSubscriptionId
            | Error::InvalidRequestId
            | Error::UnregisteredNotification(_)
            | Error::DuplicateRequestId
            | Error::RequestTimeout
            | Error::MaxSlotsExceeded => Severity::Transient,
            Error::MethodAlreadyRegistered(_)
            | Error::MethodNotFound(_)
            | Error::SubscriptionNameConflict(_)
            | Error::AlreadyStopped
            | Error::EmptyAllowList(_)
            | Error::HttpHeaderRejected(..)
            | Error::Custom(_)
            | Error::HttpNotImplemented
            | Error::EmptyBatchRequest => Severity::Permanent,
        }
    }
}

impl Triage for prost::DecodeError {
    #[inline]
    fn severity(&self) -> Severity {
        Severity::Permanent
    }
}

#[cfg(feature = "rdkafka")]
impl Triage for rdkafka::error::RDKafkaErrorCode {
    #[inline]
    fn severity(&self) -> Severity {
        Severity::Transient
    }
}

#[cfg(feature = "rdkafka")]
impl Triage for rdkafka::error::KafkaError {
    #[inline]
    fn severity(&self) -> Severity {
        use rdkafka::error::KafkaError;

        if let Some(code) = self.rdkafka_error_code() {
            return code.severity();
        }

        match self {
            KafkaError::NoMessageReceived | KafkaError::PartitionEOF(_) => Severity::Transient,
            KafkaError::Transaction(e) if e.is_retriable() => Severity::Transient,
            KafkaError::Nul(_) => Severity::Fatal,
            _ => Severity::Permanent,
        }
    }
}

impl Triage for reqwest::Error {
    #[inline]
    fn severity(&self) -> Severity {
        if self.is_status()
            && self.status().map_or(false, |s| {
                s.is_informational() || s.is_success() || s.is_client_error()
            })
            || self.is_timeout()
            || self.is_connect()
            || self.is_body()
            || self.is_decode()
        {
            Severity::Transient
        } else {
            Severity::Permanent
        }
    }
}

#[cfg(feature = "sea-orm")]
impl Triage for sea_orm::error::DbErr {
    #[inline]
    fn severity(&self) -> Severity {
        Severity::Transient
    }
}

#[cfg(feature = "solana-client")]
impl Triage for solana_client::client_error::ClientError {
    #[inline]
    fn severity(&self) -> Severity {
        use solana_client::{
            client_error::ClientErrorKind,
            rpc_request::{RpcError, RpcResponseErrorData},
        };
        use solana_sdk::transaction::TransactionError;

        match self.kind {
            ClientErrorKind::Io(ref i) => i.severity(),
            ClientErrorKind::Reqwest(ref r) => r.severity(),
            ClientErrorKind::RpcError(
                RpcError::RpcRequestError(_)
                | RpcError::RpcResponseError {
                    data: RpcResponseErrorData::NodeUnhealthy { .. },
                    ..
                },
            )
            | ClientErrorKind::TransactionError(TransactionError::ClusterMaintenance) => {
                Severity::Transient
            },
            ClientErrorKind::RpcError(
                RpcError::RpcResponseError { .. } | RpcError::ParseError(_) | RpcError::ForUser(_),
            )
            | ClientErrorKind::SerdeJson(_)
            | ClientErrorKind::SigningError(_)
            | ClientErrorKind::TransactionError(_)
            | ClientErrorKind::FaucetError(_)
            | ClientErrorKind::Custom(_) => Severity::Permanent,
        }
    }
}

#[cfg(feature = "solana-sdk")]
impl Triage for solana_sdk::pubkey::ParsePubkeyError {
    #[inline]
    fn severity(&self) -> Severity {
        Severity::Permanent
    }
}

#[cfg(feature = "solana-sdk")]
impl Triage for solana_sdk::signature::ParseSignatureError {
    #[inline]
    fn severity(&self) -> Severity {
        Severity::Permanent
    }
}

impl Triage for uuid::Error {
    #[inline]
    fn severity(&self) -> Severity {
        Severity::Permanent
    }
}

impl<'a> Error for Box<dyn Triage + 'a> {
    #[inline]
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        <dyn Triage>::source(self.as_ref())
    }

    #[inline]
    fn description(&self) -> &str {
        #[allow(deprecated)]
        <dyn Triage>::description(self.as_ref())
    }

    #[inline]
    fn cause(&self) -> Option<&dyn Error> {
        #[allow(deprecated)]
        <dyn Triage>::cause(self.as_ref())
    }
}

impl<'a> Error for Box<dyn Triage + Send + Sync + 'a> {
    #[inline]
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        <dyn Triage>::source(self.as_ref())
    }

    #[inline]
    fn description(&self) -> &str {
        #[allow(deprecated)]
        <dyn Triage>::description(self.as_ref())
    }

    #[inline]
    fn cause(&self) -> Option<&dyn Error> {
        #[allow(deprecated)]
        <dyn Triage>::cause(self.as_ref())
    }
}

impl<'a> Triage for Box<dyn Triage + 'a> {
    #[inline]
    fn severity(&self) -> Severity {
        <dyn Triage>::severity(self.as_ref())
    }
}

impl<'a> Triage for Box<dyn Triage + Send + Sync + 'a> {
    #[inline]
    fn severity(&self) -> Severity {
        <dyn Triage>::severity(self.as_ref())
    }
}

impl<T: Triage> Triage for Box<T> {
    #[inline]
    fn severity(&self) -> Severity {
        T::severity(self)
    }
}
