use std::{
    env::current_dir,
    fmt,
    fs::{self, File},
    io::{Read, Write},
    path::PathBuf,
    str::FromStr,
};

use anyhow::{Context, Result};
use directories::UserDirs;
use penumbra_chain::genesis::{self, AppState};
use penumbra_crypto::{
    keys::{SpendKey, SpendKeyBytes},
    rdsa::{SigningKey, SpendAuth, VerificationKey},
    Address,
};
use rand::Rng;
use rand_core::OsRng;
use regex::{Captures, Regex};
use serde::{de, Deserialize};
use tendermint::{node::Id, Genesis, PrivateKey};
use tendermint_config::{NodeKey, PrivValidatorKey};

/// Methods and types used for generating testnet configurations.

pub fn parse_allocations(input: impl Read) -> Result<Vec<genesis::Allocation>> {
    let mut rdr = csv::Reader::from_reader(input);
    let mut res = vec![];
    for (line, result) in rdr.deserialize().enumerate() {
        let record: TestnetAllocation = result?;
        let record: genesis::Allocation = record
            .try_into()
            .with_context(|| format!("invalid address in entry {} of allocations file", line))?;
        res.push(record);
    }

    Ok(res)
}

pub fn parse_validators(input: impl Read) -> Result<Vec<TestnetValidator>> {
    Ok(serde_json::from_reader(input)?)
}

fn string_u64<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: de::Deserializer<'de>,
{
    struct U64StringVisitor;

    impl<'de> de::Visitor<'de> for U64StringVisitor {
        type Value = u64;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a string containing a u64 with optional underscores")
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            let r = v.replace('_', "");
            r.parse::<u64>().map_err(E::custom)
        }

        fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(v)
        }
    }

    deserializer.deserialize_any(U64StringVisitor)
}

/// Hardcoded Tendermint config template. Should produce tendermint config similar to
/// https://github.com/tendermint/tendermint/blob/6291d22f46f4c4f9121375af700dbdafa51577e7/cmd/tendermint/commands/init.go#L45
/// There exists https://github.com/informalsystems/tendermint-rs/blob/a12118978f2ffea4042d6d38ebfb290d12611314/config/src/config.rs#L23 but
/// this seemed more straightforward as only the moniker is changed right now.
pub fn generate_tm_config(node_name: &str, persistent_peers: &[(Id, String)]) -> String {
    let peers_string = persistent_peers
        .iter()
        // https://docs.tendermint.com/master/spec/p2p/peer.html#peer-identity
        // Tendermint peers are expected to maintain long-term persistent identities
        // in the form of a public key. Each peer has an ID defined as
        // peer.ID == peer.PubKey.Address(), where Address uses the scheme defined in
        // crypto package.
        // the peer addresses need to match this impl: https://github.com/tendermint/tendermint/blob/f2a8f5e054cf99ebe246818bb6d71f41f9a30faa/internal/p2p/address.go#L43
        // The ID is for the node being connected to, *not* the connecting node's ID.
        .map(|(id, ip)| format!("{}@{}:26656", id, ip))
        .collect::<Vec<String>>()
        .join(",");
    format!(
        include_str!("../../testnets/tm_config_template.toml"),
        node_name, peers_string,
    )
}
pub struct ValidatorKeys {
    // Penumbra spending key and viewing key for this node.
    pub validator_id_sk: SigningKey<SpendAuth>,
    pub validator_id_vk: VerificationKey<SpendAuth>,
    // Consensus key for tendermint.
    pub validator_cons_sk: tendermint::PrivateKey,
    pub validator_cons_pk: tendermint::PublicKey,
    // P2P auth key for tendermint.
    pub node_key_sk: tendermint::PrivateKey,
    #[allow(unused_variables, dead_code)]
    pub node_key_pk: tendermint::PublicKey,
    pub validator_spend_key: SpendKeyBytes,
}

impl ValidatorKeys {
    pub fn generate() -> Self {
        // Create the spend key for this node.
        // TODO: change to use seed phrase
        let seed = SpendKeyBytes(OsRng.gen());
        let spend_key = SpendKey::from(seed.clone());

        // Create signing key and verification key for this node.
        let validator_id_sk = spend_key.spend_auth_key();
        let validator_id_vk = VerificationKey::from(validator_id_sk);

        // generate consensus key for tendermint.
        let validator_cons_sk =
            tendermint::PrivateKey::Ed25519(ed25519_consensus::SigningKey::new(OsRng));
        let validator_cons_pk = validator_cons_sk.public_key();

        // generate P2P auth key for tendermint.
        let node_key_sk =
            tendermint::PrivateKey::Ed25519(ed25519_consensus::SigningKey::new(OsRng));
        let node_key_pk = node_key_sk.public_key();

        ValidatorKeys {
            validator_id_sk: validator_id_sk.clone(),
            validator_id_vk,
            validator_cons_sk,
            validator_cons_pk,
            node_key_sk,
            node_key_pk,
            validator_spend_key: seed,
        }
    }
}

/// Represents initial allocations to the testnet.
#[derive(Debug, Deserialize)]
pub struct TestnetAllocation {
    #[serde(deserialize_with = "string_u64")]
    pub amount: u64,
    pub denom: String,
    pub address: String,
}

/// Represents a funding stream within a testnet configuration file.
#[derive(Debug, Deserialize)]
pub struct TestnetFundingStream {
    pub rate_bps: u16,
    pub address: String,
}

/// Represents testnet validators in configuration files.
#[derive(Debug, Deserialize)]
pub struct TestnetValidator {
    pub name: String,
    pub website: String,
    pub description: String,
    pub funding_streams: Vec<TestnetFundingStream>,
    pub sequence_number: u32,
}

impl TryFrom<TestnetAllocation> for genesis::Allocation {
    type Error = anyhow::Error;

    fn try_from(a: TestnetAllocation) -> anyhow::Result<genesis::Allocation> {
        Ok(genesis::Allocation {
            amount: a.amount,
            denom: a.denom.clone(),
            address: Address::from_str(&a.address)
                .context("invalid address format in genesis allocations")?,
        })
    }
}

#[derive(Deserialize)]
pub struct TendermintNodeKey {
    pub id: String,
    pub priv_key: TendermintPrivKey,
}

#[derive(Deserialize)]
pub struct TendermintPrivKey {
    #[serde(rename(serialize = "type"))]
    pub key_type: String,
    pub value: PrivateKey,
}

// Easiest to hardcode since we never change these.
pub fn get_validator_state() -> String {
    r#"{
    "height": "0",
    "round": 0,
    "step": 0
}
"#
    .to_string()
}

/// Expand tildes in a path.
/// Modified from https://stackoverflow.com/a/68233480
pub fn canonicalize_path(input: &str) -> PathBuf {
    let tilde = Regex::new(r"^~(/|$)").unwrap();
    if input.starts_with('/') {
        // if the input starts with a `/`, we use it as is
        input.into()
    } else if tilde.is_match(input) {
        // if the input starts with `~` as first token, we replace
        // this `~` with the user home directory
        PathBuf::from(&*tilde.replace(input, |c: &Captures| {
            if let Some(user_dirs) = UserDirs::new() {
                format!("{}{}", user_dirs.home_dir().to_string_lossy(), &c[1],)
            } else {
                c[0].to_string()
            }
        }))
    } else {
        PathBuf::from(format!("{}/{}", current_dir().unwrap().display(), input))
    }
}

pub fn write_configs(
    node_dir: PathBuf,
    vk: &ValidatorKeys,
    genesis: &Genesis<AppState>,
    tm_config: String,
) -> anyhow::Result<()> {
    let mut pd_dir = node_dir.clone();
    let mut tm_dir = node_dir;

    pd_dir.push("pd");
    tm_dir.push("tendermint");

    let mut node_config_dir = tm_dir.clone();
    node_config_dir.push("config");

    let mut node_data_dir = tm_dir.clone();
    node_data_dir.push("data");

    fs::create_dir_all(&node_config_dir)?;
    fs::create_dir_all(&node_data_dir)?;
    fs::create_dir_all(&pd_dir)?;

    let mut genesis_file_path = node_config_dir.clone();
    genesis_file_path.push("genesis.json");
    tracing::info!(genesis_file_path = %genesis_file_path.display(), "writing genesis");
    let mut genesis_file = File::create(genesis_file_path)?;
    genesis_file.write_all(serde_json::to_string_pretty(&genesis)?.as_bytes())?;

    let mut config_file_path = node_config_dir.clone();
    config_file_path.push("config.toml");
    tracing::info!(config_file_path = %config_file_path.display(), "writing tendermint config.toml");
    let mut config_file = File::create(config_file_path)?;
    config_file.write_all(tm_config.as_bytes())?;

    // Write this node's node_key.json
    // the underlying type doesn't implement Copy or Clone (for the best)
    let priv_key =
        tendermint::PrivateKey::Ed25519(vk.node_key_sk.ed25519_signing_key().unwrap().clone());
    let node_key = NodeKey { priv_key };
    let mut node_key_file_path = node_config_dir.clone();
    node_key_file_path.push("node_key.json");
    tracing::info!(node_key_file_path = %node_key_file_path.display(), "writing node key file");
    let mut node_key_file = File::create(node_key_file_path)?;
    node_key_file.write_all(serde_json::to_string_pretty(&node_key)?.as_bytes())?;

    // Write this node's priv_validator_key.json
    let address: tendermint::account::Id = vk.validator_cons_pk.into();
    // the underlying type doesn't implement Copy or Clone (for the best)
    let priv_key = tendermint::PrivateKey::Ed25519(
        vk.validator_cons_sk.ed25519_signing_key().unwrap().clone(),
    );
    let priv_validator_key = PrivValidatorKey {
        address,
        pub_key: vk.validator_cons_pk,
        priv_key,
    };
    let mut priv_validator_key_file_path = node_config_dir.clone();
    priv_validator_key_file_path.push("priv_validator_key.json");
    tracing::info!(priv_validator_key_file_path = %priv_validator_key_file_path.display(), "writing validator private key");
    let mut priv_validator_key_file = File::create(priv_validator_key_file_path)?;
    priv_validator_key_file
        .write_all(serde_json::to_string_pretty(&priv_validator_key)?.as_bytes())?;

    // Write the initial validator state:
    let mut priv_validator_state_file_path = node_data_dir.clone();
    priv_validator_state_file_path.push("priv_validator_state.json");
    tracing::info!(priv_validator_state_file_path = %priv_validator_state_file_path.display(), "writing validator state");
    let mut priv_validator_state_file = File::create(priv_validator_state_file_path)?;
    priv_validator_state_file.write_all(get_validator_state().as_bytes())?;

    // Write the validator's spend key:
    let mut validator_spend_key_file_path = node_config_dir.clone();
    validator_spend_key_file_path.push("validator_spend_key.json");
    tracing::info!(validator_spend_key_file_path = %validator_spend_key_file_path.display(), "writing validator spend key");
    let mut validator_spend_key_file = File::create(validator_spend_key_file_path)?;
    validator_spend_key_file
        .write_all(serde_json::to_string_pretty(&vk.validator_spend_key)?.as_bytes())?;

    Ok(())
}
