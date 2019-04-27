//! Byte-order safe and lightweight Web3 client.
//!
//! Rust-web3 has its problems because it uses ethereum-types which does not
//! work on big endian. We can do better than that just crafting our own
//! JSONRPC requests.
//!
use crate::jsonrpc::client::{Client, HTTPClient};
use crate::types::{Block, Log, NewFilter, TransactionRequest, TransactionResponse};
use clarity::abi::{derive_signature, encode_call, Token};
use clarity::utils::bytes_to_hex_str;
use clarity::{Address, PrivateKey, Transaction};
use failure::Error;
use futures::stream;
use futures::IntoFuture;
use futures::{Future, Stream};
use futures_timer::Interval;
use num256::Uint256;
use std::sync::Arc;
use std::time::Duration;
use types::Data;

fn bytes_to_data(s: &[u8]) -> String {
    let mut foo = "0x".to_string();
    foo.push_str(&bytes_to_hex_str(&s));
    foo
}

/// An instance of Web3Client.
#[derive(Clone)]
pub struct Web3 {
    jsonrpc_client: Arc<Box<HTTPClient>>,
}

impl Web3 {
    pub fn new(url: &str) -> Self {
        Self {
            jsonrpc_client: Arc::new(Box::new(HTTPClient::new(url))),
        }
    }

    pub fn eth_accounts(&self) -> Box<Future<Item = Vec<Address>, Error = Error>> {
        self.jsonrpc_client
            .request_method("eth_accounts", Vec::<String>::new())
    }
    pub fn net_version(&self) -> Box<Future<Item = String, Error = Error>> {
        self.jsonrpc_client
            .request_method("net_version", Vec::<String>::new())
    }
    pub fn eth_new_filter(
        &self,
        new_filter: NewFilter,
    ) -> Box<Future<Item = Uint256, Error = Error>> {
        self.jsonrpc_client
            .request_method("eth_newFilter", vec![new_filter])
    }
    pub fn eth_get_filter_changes(
        &self,
        filter_id: Uint256,
    ) -> Box<Future<Item = Vec<Log>, Error = Error>> {
        self.jsonrpc_client.request_method(
            "eth_getFilterChanges",
            vec![format!("{:#x}", filter_id.clone())],
        )
    }
    pub fn eth_uninstall_filter(
        &self,
        filter_id: Uint256,
    ) -> Box<Future<Item = bool, Error = Error>> {
        self.jsonrpc_client.request_method(
            "eth_uninstallFilter",
            vec![format!("{:#x}", filter_id.clone())],
        )
    }

    pub fn eth_get_logs(
        &self,
        new_filter: NewFilter,
    ) -> Box<Future<Item = Vec<Log>, Error = Error>> {
        self.jsonrpc_client
            .request_method("eth_getLogs", vec![new_filter])
    }

    pub fn eth_get_transaction_count(
        &self,
        address: Address,
    ) -> Box<Future<Item = Uint256, Error = Error>> {
        self.jsonrpc_client.request_method(
            "eth_getTransactionCount",
            vec![address.to_string(), "latest".to_string()],
        )
    }
    pub fn eth_gas_price(&self) -> Box<Future<Item = Uint256, Error = Error>> {
        self.jsonrpc_client
            .request_method("eth_gasPrice", Vec::<String>::new())
    }
    pub fn eth_get_balance(&self, address: Address) -> Box<Future<Item = Uint256, Error = Error>> {
        self.jsonrpc_client.request_method(
            "eth_getBalance",
            vec![address.to_string(), "latest".to_string()],
        )
    }
    pub fn eth_send_transaction(
        &self,
        transactions: Vec<TransactionRequest>,
    ) -> Box<Future<Item = Uint256, Error = Error>> {
        self.jsonrpc_client
            .request_method("eth_sendTransaction", transactions)
    }
    pub fn eth_call(
        &self,
        transaction: TransactionRequest,
    ) -> Box<Future<Item = Data, Error = Error>> {
        self.jsonrpc_client
            .request_method("eth_call", vec![transaction])
    }
    pub fn eth_block_number(&self) -> Box<Future<Item = Uint256, Error = Error>> {
        self.jsonrpc_client
            .request_method("eth_blockNumber", Vec::<String>::new())
    }

    pub fn eth_get_block_by_number(
        &self,
        block_number: Uint256,
    ) -> Box<Future<Item = Block, Error = Error>> {
        self.jsonrpc_client
            .request_method("eth_getBlockByNumber", block_number)
    }

    pub fn eth_send_raw_transaction(
        &self,
        data: Vec<u8>,
    ) -> Box<Future<Item = Uint256, Error = Error>> {
        self.jsonrpc_client.request_method(
            "eth_sendRawTransaction",
            vec![format!("0x{}", bytes_to_hex_str(&data))],
        )
    }
    pub fn eth_get_transaction_by_hash(
        &self,
        hash: Uint256,
    ) -> Box<Future<Item = Option<TransactionResponse>, Error = Error>> {
        self.jsonrpc_client.request_method(
            "eth_getTransactionByHash",
            /// XXX: Technically it doesn't need to be Uint256, but since send_raw_transaction is
            /// returning it we'll keep it consistent.
            vec![format!("{:#066x}", hash)],
        )
    }
    pub fn evm_snapshot(&self) -> Box<Future<Item = Uint256, Error = Error>> {
        self.jsonrpc_client
            .request_method("evm_snapshot", Vec::<String>::new())
    }
    pub fn evm_revert(&self, snapshot_id: Uint256) -> Box<Future<Item = Uint256, Error = Error>> {
        self.jsonrpc_client
            .request_method("evm_revert", vec![format!("{:#066x}", snapshot_id)])
    }

    /// Sends a transaction which changes blockchain state.
    pub fn send_transaction(
        &self,
        to_address: Address,
        data: Vec<u8>,
        value: Uint256,
        own_address: Address,
        secret: PrivateKey,
    ) -> Box<Future<Item = Uint256, Error = Error>> {
        let salf = self.clone();
        let props = self
            .eth_gas_price()
            .join(self.eth_get_transaction_count(own_address));

        Box::new(
            props
                .and_then(move |(gas_price, nonce)| {
                    let transaction = Transaction {
                        to: to_address,
                        nonce: nonce,
                        gas_price: gas_price.into(),
                        gas_limit: 6721975u32.into(),
                        value,
                        data,
                        signature: None,
                    };

                    let transaction = transaction.sign(&secret, Some(1u64));

                    salf.eth_send_raw_transaction(
                        transaction
                            .to_bytes()
                            .expect("transaction.to_bytes() failed"),
                    )
                })
                .into_future(),
        )
    }

    /// Sends a transaction which does not change blockchain state, usually to get information.
    pub fn contract_call(
        &self,
        contract_address: Address,
        sig: &str,
        tokens: &[Token],
        own_address: Address,
    ) -> Box<Future<Item = Uint256, Error = Error>> {
        let salf = self.clone();

        let props = salf
            .eth_gas_price()
            .join(salf.eth_get_transaction_count(own_address));

        let payload = encode_call(sig, tokens);

        Box::new(
            props
                .and_then(move |(gas_price, nonce)| {
                    let transaction = TransactionRequest {
                        from: own_address,
                        to: Some(contract_address),
                        nonce: Some(nonce),
                        gas: None,
                        gas_price: gas_price.into(),
                        value: Some(0u64.into()),
                        data: Some(Data(payload)),
                    };

                    salf.eth_call(transaction)
                })
                .and_then(|bytes| Ok(Uint256::from_bytes_be(&bytes))),
        )
    }

    /// Checks if an event has already happened.
    pub fn check_for_event(
        &self,
        contract_address: Address,
        event: &str,
        topic1: Option<Vec<[u8; 32]>>,
        topic2: Option<Vec<[u8; 32]>>,
    ) -> Box<Future<Item = Option<Log>, Error = Error>> {
        let salf = self.clone();

        // Build a filter with specified topics
        let mut new_filter = NewFilter::default();
        new_filter.address = vec![contract_address.clone()];
        new_filter.topics = Some(vec![
            Some(vec![Some(bytes_to_data(&derive_signature(event)))]),
            topic1.map(|v| v.into_iter().map(|val| Some(bytes_to_data(&val))).collect()),
            topic2.map(|v| v.into_iter().map(|val| Some(bytes_to_data(&val))).collect()),
        ]);

        Box::new(salf.eth_get_logs(new_filter).and_then(|logs| {
            // Assuming the latest log is at the head of the vec
            Ok(logs.first().map(|log| log.clone()))
        }))
    }

    /// Waits for a transaction with the given hash to show up on the chain
    pub fn wait_for_transaction(
        &self,
        tx_hash: [u8; 32],
    ) -> Box<Future<Item = TransactionResponse, Error = Error>> {
        let salf = self.clone();
        let fut = Interval::new(Duration::from_secs(1))
            .from_err()
            .and_then(move |_| salf.eth_get_transaction_by_hash(tx_hash.into()))
            .filter_map(move |maybe_tx| maybe_tx)
            .into_future()
            .map(|(v, _stream)| v.unwrap())
            .map_err(|(e, _stream)| e);

        Box::new(fut)
    }

    /// Sets up an event filter, waits for the event to happen, then removes the filter. Includes a
    /// local filter. If a captured event does not pass this filter, it is ignored.
    pub fn wait_for_event<F: Fn(Log) -> bool + 'static>(
        &self,
        contract_address: Address,
        event: &str,
        topic1: Option<Vec<[u8; 32]>>,
        topic2: Option<Vec<[u8; 32]>>,
        topic3: Option<Vec<[u8; 32]>>,
        local_filter: F,
    ) -> Box<Future<Item = Log, Error = Error>> {
        let salf = self.clone();

        let mut new_filter = NewFilter::default();
        new_filter.address = vec![contract_address.clone()];
        new_filter.from_block = None;
        new_filter.to_block = None;
        new_filter.topics = Some(vec![
            Some(vec![Some(bytes_to_data(&derive_signature(event)))]),
            topic1.map(|v| v.into_iter().map(|val| Some(bytes_to_data(&val))).collect()),
            topic2.map(|v| v.into_iter().map(|val| Some(bytes_to_data(&val))).collect()),
            topic3.map(|v| v.into_iter().map(|val| Some(bytes_to_data(&val))).collect()),
        ]);

        Box::new(salf.eth_new_filter(new_filter).and_then(move |filter_id| {
            Interval::new(Duration::from_secs(2))
                .from_err()
                .and_then({
                    let filter_id = filter_id.clone();
                    let salf = salf.clone();
                    move |_| salf.eth_get_filter_changes(filter_id.clone())
                })
                .filter_map(move |logs: Vec<Log>| {
                    for log in logs {
                        if local_filter(log.clone()) {
                            return Some(log);
                        }
                    }

                    None
                })
                .into_future()
                .map(|(v, _stream)| v.unwrap())
                .map_err(|(e, _stream)| e)
                .and_then(move |log| {
                    salf.eth_uninstall_filter(filter_id).and_then(move |r| {
                        ensure!(r, "Unable to properly uninstall filter");
                        Ok(log)
                    })
                })
        }))
    }
}
