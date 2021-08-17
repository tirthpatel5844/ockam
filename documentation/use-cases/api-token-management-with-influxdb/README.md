# Using Leases to manage InfluxDB API token

In this guide we'll show how you can use Ockam to securely distribute and manage InfluxDB API Tokens, and control access
with features like revocation and expiration.

The Ockam Lease Manager is a service that runs in an Ockam Cloud Node. The Lease Manager integration with InfluxDB enables
a Cloud Node to create, update and revoke API tokens for an InfluxDB instance. A lease is like an envelope around an API
token, adding additional authorization information.

Devices in the field obtain InfluxDB API tokens from the Lease Manager using a secure channel. When the lease expires, or
the underlying API token has been revoked, the device will request a new lease.

This dynamic provisioning of API tokens allows scalable and secure management of credentials over time. If an API token
becomes compromised, it can simply be revoked. The Lease Manager will provision a new token to the devices.

# Example

Let's build a Rust program that sends data to InfluxDB using a leased API token. We will:

- set up an Ockam Cloud Node and configure the InfluxDB integration
- create an Ockam Node in Rust for the device
- establish a secure channel between the Device Node to the Cloud Node
- obtain a lease for an API token
- send data from the Node to an InfluxDB instance using the leased API token
- demonstrate lease expiration

## Rust Setup

If you don't have it, please [install](https://www.rust-lang.org/tools/install) the latest version of Rust.

```
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Next, create a new cargo project to get started:

```
cargo new --lib ockam_influxdb && cd ockam_influxdb
```

Add the following dependencies:

```toml
[dependencies]
ockam = "0"
ockam_transport_tcp = "0"
reqwest = { version = "0", features = ["json"] }
rand = "0"
```

## InfluxDB Client

In this example, we will use the `reqwest` crate to build a small InfluxDB v2.0 HTTPS client. This client writes 10
random values into a `metrics` measurement.

Create a new file named `src/influx_client.rs`.  Paste the below content into the file:

```rust
use rand::random;
use reqwest::header::{HeaderMap, HeaderValue};
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Represents potential InfluxDB errors. Specifically, we are interested in categorizing authentication
/// errors distinctly from other errors. This allows us to take specific actions, such as revoking a lease.
#[derive(Debug, Clone)]
pub enum InfluxError {
    Authentication(String),
    Unknown,
}

impl Display for InfluxError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            InfluxError::Authentication(msg) => f.write_str(msg),
            _ => f.write_str("Unknown"),
        }
    }
}

impl Error for InfluxError {}

impl InfluxError {
    pub fn is_authentication_error(&self) -> bool {
        matches!(self, InfluxError::Authentication(_))
    }
}

/// A basic InfluxDB client. Contains InfluxDB meta-data and a leased token.
pub struct InfluxClient {
    api_url: String,
    org: String,
    bucket: String,
    leased_token: String,
}

impl InfluxClient {
    /// Create a new client.
    pub fn new(api_url: &str, org: &str, bucket: &str, leased_token: &str) -> Self {
        InfluxClient {
            api_url: api_url.to_string(),
            org: org.to_string(),
            bucket: bucket.to_string(),
            leased_token: leased_token.to_string(),
        }
    }

    /// Set the current token.
    pub fn set_token(&mut self, leased_token: &str) {
        self.leased_token = leased_token.to_string();
    }

    /// Send some random metrics to InfluxDB.
    pub async fn send_metrics(&self) -> Result<(), InfluxError> {
        let url = format!(
            "{}/api/v2/write?org={}&bucket={}&precision=s",
            self.api_url, self.org, self.bucket
        );

        let mut headers = HeaderMap::new();
        let token = format!("Token {}", self.leased_token);

        headers.insert(
            "Authorization",
            HeaderValue::from_str(token.as_str()).unwrap(),
        );

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .unwrap();

        // Send 10 data points. On authentication error (403), return an `InfluxError::Authentication`
        for i in 0..10 {
            let data = random::<usize>() % 10_000;
            let metric = format!("metrics,env=test r{}={}", i, data);
            let resp = client.post(url.clone()).body(metric).send().await.unwrap();
            if resp.status().as_u16() == 403 {
                return Err(InfluxError::Authentication(
                    "Authentication failed.".to_string(),
                ));
            }
        }
        Ok(())
    }
}

```


Let's expose this client by re-exporting from `lib.rs`.  Create `src/lib.rs` and paste:

```rust
mod influx_client;
pub use influx_client::*;
```


## Device Node

Now that we have an InfluxDB client to work with, we can use it in a node.

Create `src/bin/client.rs` and add the following:

```rust
use ockam::{route, Context, Entity, Identity, Result, TcpTransport, Vault, TCP};
// TODO: Add these use when we switch to secure channel
// use ockam::{SecureChannels, TrustEveryonePolicy}

use ockam_influxdb::InfluxClient;
use std::io::Read;
use std::thread::sleep;
use std::time::Duration;

#[ockam::node]
async fn main(ctx: Context) -> Result<()> {
    let _tcp = TcpTransport::create(&ctx).await?;

    let vault = Vault::create(&ctx)?;
    let mut entity = Entity::create(&ctx, &vault)?;
    // TODO: Uncomment when secure channel is available on Hub
    // Create a secure channel
    /*
        let secure_channel_route = route![(TCP, "your.ockam.node:4100"), "secure_channel"];
        let secure_channel = entity.create_secure_channel(secure_channel_route, TrustEveryonePolicy)?;
    */

    // InfluxDB details
    let api_url = "http://your.influx.instance:8086";
    let org = "0123456789abcdef";
    let bucket = "your-bucket";

    // Get an API token from the Token Lease Service
    // TODO: Use this route when secure channel is available
    // let lease_route = route![secure_channel, "influxdb_token_lease_service"];
    let lease_route = route![
        (TCP, "your.ockam.node:4100"),
        "influxdb_token_lease_service"
    ];

    let leased_token = entity.get_lease(&lease_route, org)?;

    // Create the InfluxDB client using the leased token
    let mut influx_client = InfluxClient::new(api_url, org, bucket, leased_token.value());

    // Write data once per second. On authentication failure, request a new lease.
    loop {
        let response = influx_client.send_metrics().await;
        if let Err(influx_error) = response {
            if influx_error.is_authentication_error() {
                println!("Authentication failed. Revoking lease.");
                entity.revoke_lease(&lease_route, leased_token.clone())?;

                // Interactively pause. This allows an opportunity to verify the API token status globally.
                println!("Press enter to get a new lease");
                std::io::stdin().read_exact(&mut [0_u8; 1]).unwrap();

                // Get a new lease
                let leased_token = entity.get_lease(&lease_route, org)?;

                // Update the client
                influx_client.set_token(leased_token.value());
            }
        }
        sleep(Duration::from_secs(1));
    }
}
```

# Conclusion