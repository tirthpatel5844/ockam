use crate::{Changes, Contact, Lease, ProfileChangeEvent, ProfileIdentifier, TrustPolicy, TTL};
use ockam_core::compat::{string::String, vec::Vec};
use ockam_core::{async_trait, compat::boxed::Box, AsyncTryClone};
use ockam_core::{Address, Result, Route};
use ockam_vault_core::{PublicKey, Secret};

pub type AuthenticationProof = Vec<u8>;

/// Identity
#[async_trait]
pub trait Identity: AsyncTryClone + Send + Sync + 'static {
    /// Return unique [`Profile`] identifier, which is equal to sha256 of the root public key
    async fn identifier(&self) -> Result<ProfileIdentifier>;

    /// Create new key.
    async fn create_key(&mut self, label: String) -> Result<()>;

    /// Rotate existing key.
    async fn rotate_profile_key(&mut self) -> Result<()>;

    /// Get [`Secret`] key.
    async fn get_profile_secret_key(&self) -> Result<Secret>;

    /// Get [`Secret`] key.
    async fn get_secret_key(&self, label: String) -> Result<Secret>;

    /// Get [`PublicKey`].
    async fn get_profile_public_key(&self) -> Result<PublicKey>;

    /// Get [`PublicKey`].
    async fn get_public_key(&self, label: String) -> Result<PublicKey>;

    /// Create an authentication proof based on the given state
    async fn create_auth_proof(&mut self, state_slice: &[u8]) -> Result<AuthenticationProof>;

    /// Verify a proof based on the given state, proof and profile.
    async fn verify_auth_proof(
        &mut self,
        state_slice: &[u8],
        peer_id: &ProfileIdentifier,
        proof_slice: &[u8],
    ) -> Result<bool>;

    /// Add a change event.
    async fn add_change(&mut self, change_event: ProfileChangeEvent) -> Result<()>;

    /// Return change history chain
    async fn get_changes(&self) -> Result<Changes>;

    /// Verify the whole change event chain
    async fn verify_changes(&mut self) -> Result<bool>;

    /// Return all known to this profile [`Contact`]s
    async fn get_contacts(&self) -> Result<Vec<Contact>>;

    /// Convert [`Profile`] to [`Contact`]
    async fn as_contact(&mut self) -> Result<Contact>;

    /// Return [`Contact`] with given [`ProfileIdentifier`]
    async fn get_contact(&mut self, contact_id: &ProfileIdentifier) -> Result<Option<Contact>>;

    /// Verify cryptographically whole event chain. Also verify sequence correctness
    async fn verify_contact(&mut self, contact: Contact) -> Result<bool>;

    /// Verify and add new [`Contact`] to [`Profile`]'s Contact list
    async fn verify_and_add_contact(&mut self, contact: Contact) -> Result<bool>;

    /// Verify and update known [`Contact`] with new [`ProfileChangeEvent`]s
    async fn verify_and_update_contact(
        &mut self,
        contact_id: &ProfileIdentifier,
        change_events: &[ProfileChangeEvent],
    ) -> Result<bool>;

    async fn get_lease(
        &self,
        lease_manager_route: &Route,
        org_id: String,
        bucket: String,
        ttl: TTL,
    ) -> Result<Lease>;

    async fn revoke_lease(&mut self, lease_manager_route: &Route, lease: Lease) -> Result<()>;
}

#[async_trait]
pub trait SecureChannels {
    async fn create_secure_channel_listener(
        &mut self,
        address: Address,
        trust_policy: impl TrustPolicy,
    ) -> Result<()>;

    async fn create_secure_channel(
        &mut self,
        route: Route,
        trust_policy: impl TrustPolicy,
    ) -> Result<Address>;
}
