// This file is part of radicle-link
// <https://github.com/radicle-dev/radicle-link>
//
// Copyright (C) 2019-2020 The Radicle Team <dev@radicle.xyz>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 or
// later as published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

use std::{fmt, iter, ops::Deref};

use bit_vec::BitVec;
use multibase::Base;
use secstr::SecStr;
use serde::{de::Visitor, Deserialize, Deserializer, Serialize, Serializer};
use sodiumoxide::crypto::sign::ed25519;
use thiserror::Error;

use keystore::SecretKeyExt;

pub use ed25519::PUBLICKEYBYTES;

/// Version of the signature scheme in use
///
/// This is used for future-proofing serialisation. For ergonomics reasons, we
/// avoid introducing single-variant enums just now, and just serialize a
/// version tag alongside the data.
const VERSION: u8 = 0;

/// A device-specific signing key
#[derive(Clone, Eq, PartialEq)]
pub struct SecretKey(ed25519::SecretKey);

/// The public part of a `Key``
#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub struct PublicKey(ed25519::PublicKey);

/// A signature produced by `Key::sign`
#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub struct Signature(ed25519::Signature);

// Key

#[allow(clippy::new_without_default)]
impl SecretKey {
    pub fn new() -> Self {
        let (_, sk) = ed25519::gen_keypair();
        Self(sk)
    }

    #[cfg(test)]
    pub fn from_seed(seed: &ed25519::Seed) -> Self {
        let (_, sk) = ed25519::keypair_from_seed(seed);
        Self(sk)
    }

    pub(crate) fn from_secret(sk: ed25519::SecretKey) -> Self {
        Self(sk)
    }

    pub fn public(&self) -> PublicKey {
        PublicKey(self.0.public_key())
    }

    pub fn sign(&self, data: &[u8]) -> Signature {
        Signature(ed25519::sign_detached(data, &self.0))
    }

    const PKCS_ED25519_OID: &'static [u64] = &[1, 3, 101, 112];

    /// Export in PKCS#8 format.
    ///
    /// **NOTE**: this will export private key material. Use with caution.
    ///
    /// Attribution: this code is stolen from the `thrussh` project.
    pub fn as_pkcs8(&self) -> Vec<u8> {
        yasna::construct_der(|writer| {
            writer.write_sequence(|writer| {
                writer.next().write_u32(1);
                writer.next().write_sequence(|writer| {
                    writer
                        .next()
                        .write_oid(&yasna::models::ObjectIdentifier::from_slice(
                            Self::PKCS_ED25519_OID,
                        ));
                });
                let seed = yasna::construct_der(|writer| writer.write_bytes(&self.0[..32]));
                writer.next().write_bytes(&seed);
                writer
                    .next()
                    .write_tagged(yasna::Tag::context(1), |writer| {
                        let public = &self.0[32..];
                        writer.write_bitvec(&BitVec::from_bytes(&public))
                    })
            })
        })
    }
}

impl fmt::Display for SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.public().fmt(f)
    }
}

impl AsRef<[u8]> for SecretKey {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

#[derive(Debug, Error)]
pub enum IntoSecretKeyError {
    #[error("Invalid length")]
    InvalidSliceLength,
}

impl SecretKeyExt for SecretKey {
    type Metadata = ();
    type Error = IntoSecretKeyError;

    fn from_bytes_and_meta(bytes: SecStr, _metadata: &Self::Metadata) -> Result<Self, Self::Error> {
        let sk = ed25519::SecretKey::from_slice(bytes.unsecure())
            .ok_or(IntoSecretKeyError::InvalidSliceLength)?;
        Ok(Self::from_secret(sk))
    }

    fn metadata(&self) -> Self::Metadata {}
}

// PublicKey

impl PublicKey {
    pub fn verify(&self, sig: &Signature, data: &[u8]) -> bool {
        ed25519::verify_detached(sig, &data, self)
    }

    pub fn from_slice(bs: &[u8]) -> Option<PublicKey> {
        ed25519::PublicKey::from_slice(&bs).map(PublicKey)
    }

    pub fn from_bs58(s: &str) -> Option<Self> {
        let bytes = match bs58::decode(s.as_bytes())
            .with_alphabet(bs58::alphabet::BITCOIN)
            .into_vec()
        {
            Ok(v) => v,
            Err(_) => return None,
        };
        ed25519::PublicKey::from_slice(&bytes).map(PublicKey)
    }

    pub fn to_bs58(&self) -> String {
        bs58::encode(self.0)
            .with_alphabet(bs58::alphabet::BITCOIN)
            .into_string()
    }
}

impl From<ed25519::PublicKey> for PublicKey {
    fn from(pk: ed25519::PublicKey) -> Self {
        Self(pk)
    }
}

impl From<SecretKey> for PublicKey {
    fn from(k: SecretKey) -> Self {
        k.public()
    }
}

impl fmt::Display for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex::encode(self.as_ref()))
    }
}

impl AsRef<[u8]> for PublicKey {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl Deref for PublicKey {
    type Target = ed25519::PublicKey;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Serialize for PublicKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        multibase::encode(
            Base::Base32Z,
            iter::once(&VERSION)
                .chain(self.as_ref())
                .cloned()
                .collect::<Vec<u8>>(),
        )
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for PublicKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct PublicKeyVisitor;

        impl<'de> Visitor<'de> for PublicKeyVisitor {
            type Value = PublicKey;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "a PublicKey, version {}", VERSION)
            }

            fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                let (_, bytes) = multibase::decode(s).map_err(serde::de::Error::custom)?;
                match bytes.split_first() {
                    // impossible, actually
                    None => Err(serde::de::Error::custom("Empty input")),
                    Some((version, data)) => {
                        if version != &VERSION {
                            return Err(serde::de::Error::custom(format!(
                                "Unknown PublicKey version {}",
                                version
                            )));
                        }

                        ed25519::PublicKey::from_slice(data).map(PublicKey).ok_or({
                            serde::de::Error::custom("Invalid length for ed25519 public key")
                        })
                    },
                }
            }
        }

        deserializer.deserialize_str(PublicKeyVisitor)
    }
}

// Signature

impl Signature {
    pub fn verify(&self, data: &[u8], pk: &PublicKey) -> bool {
        ed25519::verify_detached(self, &data, pk)
    }

    pub fn from_bs58(s: &str) -> Option<Self> {
        let bytes = match bs58::decode(s.as_bytes())
            .with_alphabet(bs58::alphabet::BITCOIN)
            .into_vec()
        {
            Ok(v) => v,
            Err(_) => return None,
        };
        sodiumoxide::crypto::sign::Signature::from_slice(&bytes).map(Self)
    }

    pub fn to_bs58(&self) -> String {
        bs58::encode(self.0)
            .with_alphabet(bs58::alphabet::BITCOIN)
            .into_string()
    }
}

impl AsRef<[u8]> for Signature {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl Deref for Signature {
    type Target = ed25519::Signature;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Serialize for Signature {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        multibase::encode(
            Base::Base32Z,
            iter::once(&VERSION)
                .chain(self.as_ref())
                .cloned()
                .collect::<Vec<u8>>(),
        )
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Signature {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct SignatureVisitor;

        impl<'de> Visitor<'de> for SignatureVisitor {
            type Value = Signature;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "a Signature, version {}", VERSION)
            }

            fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                let (_, bytes) = multibase::decode(s).map_err(serde::de::Error::custom)?;
                match bytes.split_first() {
                    // impossible, actually
                    None => Err(serde::de::Error::custom("Empty input")),
                    Some((version, data)) => {
                        if version != &VERSION {
                            return Err(serde::de::Error::custom(format!(
                                "Unknown Signature version {}",
                                version
                            )));
                        }

                        ed25519::Signature::from_slice(data).map(Signature).ok_or({
                            serde::de::Error::custom("Invalid length for ed25519 signature")
                        })
                    },
                }
            }
        }

        deserializer.deserialize_str(SignatureVisitor)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    const DATA_TO_SIGN: &[u8] = b"alors monsieur";

    #[test]
    fn test_sign_verify_via_signature() {
        let key = SecretKey::new();
        let sig = key.sign(&DATA_TO_SIGN);
        assert!(sig.verify(&DATA_TO_SIGN, &key.public()))
    }

    #[test]
    fn test_sign_verify_via_pubkey() {
        let key = SecretKey::new();
        let sig = key.sign(&DATA_TO_SIGN);
        assert!(key.public().verify(&sig, &DATA_TO_SIGN))
    }

    #[test]
    fn test_public_key_serde() {
        let pk = SecretKey::new().public();
        assert_eq!(
            pk,
            serde_json::from_str(&serde_json::to_string(&pk).unwrap()).unwrap()
        )
    }

    #[test]
    fn test_public_key_deserialize_wrong_version() {
        let pk = SecretKey::new().public();
        let ser = multibase::encode(
            Base::Base32Z,
            iter::once(&1)
                .chain(pk.as_ref())
                .cloned()
                .collect::<Vec<u8>>(),
        );
        assert!(serde_json::from_str::<PublicKey>(&ser).is_err())
    }

    #[test]
    fn test_signature_serde() {
        let key = SecretKey::new();
        let sig = key.sign(&DATA_TO_SIGN);
        assert_eq!(
            sig,
            serde_json::from_str(&serde_json::to_string(&sig).unwrap()).unwrap()
        )
    }

    #[test]
    fn test_signature_deserialize_wrong_version() {
        let key = SecretKey::new();
        let sig = key.sign(&DATA_TO_SIGN);
        let ser = multibase::encode(
            Base::Base32Z,
            iter::once(&1)
                .chain(sig.as_ref())
                .cloned()
                .collect::<Vec<u8>>(),
        );
        assert!(serde_json::from_str::<Signature>(&ser).is_err())
    }
}
