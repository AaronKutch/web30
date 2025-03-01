//! This module contains functions for managing Ethereum events
use crate::{client::Web3, types::NewFilter};
use crate::{jsonrpc::error::Web3Error, types::Log};
use clarity::{
    abi::{derive_signature, SerializedToken, Token},
    utils::bytes_to_hex_str,
};
use clarity::{Address, Uint256};
use std::time::{Duration, Instant};
use tokio::time::sleep as delay_for;

/// takes an address and spits out an event, There's some argument to
/// not use [u8; 32] for event definitions because of how much trouble
/// this is but mostly you watch for addresses, Uint256 values and byte arrays
/// the last is best represented by this and the first is easily provided by this
/// function. Uint256 has a get_bytes function but you do need to check the length
pub fn address_to_event(address: Address) -> [u8; 32] {
    let token = Token::Address(address);
    match token.serialize() {
        SerializedToken::Dynamic(_) => panic!("Addresses are not dynamic!"),
        SerializedToken::Static(v) => v,
    }
}

fn bytes_to_data(s: &[u8]) -> String {
    let mut val = "0x".to_string();
    val.push_str(&bytes_to_hex_str(s));
    val
}

impl Web3 {
    /// Waits for a single event but instead of creating a filter and checking
    /// for changes this function waits for the provided wait time before
    /// checking if the event has occurred. This function will wait for at
    // least 'wait_time' before progressing, regardless of the outcome.
    pub async fn wait_for_event_alt<F: Fn(Log) -> bool + 'static>(
        &self,
        wait_time: Duration,
        contract_address: Vec<Address>,
        event: &str,
        topics: Vec<Vec<[u8; 32]>>,
        local_filter: F,
    ) -> Result<Log, Web3Error> {
        let sig = derive_signature(event)?;
        let mut final_topics = vec![Some(vec![Some(bytes_to_data(&sig))])];
        for topic in topics {
            let mut parts = Vec::new();
            for item in topic {
                parts.push(Some(bytes_to_data(&item)))
            }
            final_topics.push(Some(parts));
        }

        let new_filter = NewFilter {
            address: contract_address,
            from_block: None,
            to_block: None,
            topics: Some(final_topics),
        };

        delay_for(wait_time).await;
        let logs = match self.eth_get_logs(new_filter.clone()).await {
            Ok(logs) => logs,
            Err(e) => return Err(e),
        };

        for log in logs {
            if local_filter(log.clone()) {
                return Ok(log);
            }
        }
        Err(Web3Error::EventNotFound(event.to_string()))
    }

    /// Sets up an event filter, waits for a single event to happen, then removes the filter. Includes a
    /// local filter. If a captured event does not pass this filter, it is ignored. This differs from
    /// wait_for_event_alt in that it will check for filter changes every second and potentially exit
    /// earlier than the wait_for time provided by the user.
    pub async fn wait_for_event<F: Fn(Log) -> bool + 'static>(
        &self,
        wait_for: Duration,
        contract_address: Vec<Address>,
        event: &str,
        topics: Vec<Vec<[u8; 32]>>,
        local_filter: F,
    ) -> Result<Log, Web3Error> {
        let sig = derive_signature(event)?;
        let mut final_topics = vec![Some(vec![Some(bytes_to_data(&sig))])];
        for topic in topics {
            let mut parts = Vec::new();
            for item in topic {
                parts.push(Some(bytes_to_data(&item)))
            }
            final_topics.push(Some(parts));
        }

        let new_filter = NewFilter {
            address: contract_address,
            from_block: None,
            to_block: None,
            topics: Some(final_topics),
        };

        let filter_id = match self.eth_new_filter(new_filter).await {
            Ok(f) => f,
            Err(e) => return Err(e),
        };

        let start = Instant::now();
        let mut found_log = None;
        while Instant::now() - start < wait_for {
            delay_for(Duration::from_secs(1)).await;
            let logs = match self.eth_get_filter_changes(filter_id).await {
                Ok(changes) => changes,
                Err(e) => return Err(e),
            };
            for log in logs {
                if local_filter(log.clone()) {
                    found_log = Some(log);
                    break;
                }
            }
        }

        if let Err(e) = self.eth_uninstall_filter(filter_id).await {
            return Err(Web3Error::CouldNotRemoveFilter(format!("{}", e)));
        }

        match found_log {
            Some(log) => Ok(log),
            None => Err(Web3Error::EventNotFound(event.to_string())),
        }
    }

    /// Checks for multiple events as defined by their signature strings over a block range. If no ending block is provided
    /// the latest finalized block will be used. This function will not wait for events to occur.
    pub async fn check_for_events(
        &self,
        start_block: Uint256,
        end_block: Option<Uint256>,
        contract_address: Vec<Address>,
        events: Vec<&str>,
    ) -> Result<Vec<Log>, Web3Error> {
        // Build a filter with specified topics
        let from_block = Some(format!("{:#x}", start_block));
        let to_block;
        if let Some(end_block) = end_block {
            to_block = Some(format!("{:#x}", end_block));
        } else {
            let latest_block = self.eth_finalized_block_number().await?;
            to_block = Some(format!("{:#x}", latest_block));
        }

        let mut final_topics = Vec::new();
        for event in events {
            let sig = derive_signature(event)?;
            final_topics.push(Some(vec![Some(bytes_to_data(&sig))]));
        }

        let new_filter = NewFilter {
            address: contract_address,
            from_block,
            to_block,
            topics: Some(final_topics),
        };

        self.eth_get_logs(new_filter).await
    }

    /// Checks for multiple events as defined by arbitrary user input over a block range. If no ending block is provided
    /// the latest finalized block will be used. This function will not wait for events to occur
    pub async fn check_for_arbitrary_events(
        &self,
        start_block: Uint256,
        end_block: Option<Uint256>,
        contract_address: Vec<Address>,
        topics: Vec<Vec<[u8; 32]>>,
    ) -> Result<Vec<Log>, Web3Error> {
        // Build a filter with specified topics
        let from_block = Some(format!("{:#x}", start_block));
        let to_block;
        if let Some(end_block) = end_block {
            to_block = Some(format!("{:#x}", end_block));
        } else {
            let latest_block = self.eth_finalized_block_number().await?;
            to_block = Some(format!("{:#x}", latest_block));
        }

        let mut final_topics = Vec::new();
        for topic in topics {
            let mut parts = Vec::new();
            for item in topic {
                parts.push(Some(bytes_to_data(&item)))
            }
            final_topics.push(Some(parts));
        }

        let new_filter = NewFilter {
            address: contract_address,
            from_block,
            to_block,
            topics: Some(final_topics),
        };

        self.eth_get_logs(new_filter).await
    }
}
