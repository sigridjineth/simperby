use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub struct TxDelegateUpdateDelegatorResponse {
    pub updated: bool,
    pub consensus_delegatee: *const Option<MemberName>,
    pub governance_delegatee: *const Option<MemberName>,
    pub delegated_consensus_voting_power: Option<VotingPower>,
    pub delegated_governance_voting_power: Option<VotingPower>,
}

pub struct TxUndelegateUpdateDelegatorResponse {
    pub updated: bool,
    pub consensus_delegatee: Option<MemberName>,
    pub governance_delegatee: Option<MemberName>,
    pub undelegated_consensus_voting_power: Option<VotingPower>,
    pub undelegated_governance_voting_power: Option<VotingPower>,
}

/// The partial set of the blockchain state which is reserved and protected.
///
/// It is stored in the reserved directory of the repository.
/// Any transaction which modifies this state MUST produce a valid next one.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct ReservedState {
    /// The genesis info. This must never be changed.
    pub genesis_info: GenesisInfo,
    /// The members.
    pub members: Vec<Member>,
    /// The leader order of the consensus rounds.
    ///
    /// It MUST be sorted by the name of the members.
    pub consensus_leader_order: Vec<MemberName>,
    /// The semantic version of Simperby protocol for this network.
    pub version: String,
}

impl ReservedState {
    pub fn get_validator_set(&self) -> Result<Vec<(PublicKey, VotingPower)>, String> {
        let mut validator_set = HashMap::new();
        for member in &self.members {
            if let Some(delegatee) = &member.consensus_delegatee {
                validator_set
                    .entry(delegatee.clone())
                    .and_modify(|v| *v += member.consensus_voting_power)
                    .or_insert(member.consensus_voting_power);
            } else {
                validator_set
                    .entry(member.name.clone())
                    .and_modify(|v| *v += member.consensus_voting_power)
                    .or_insert(member.consensus_voting_power);
            }
        }
        Ok(validator_set
            .iter()
            .map(|(name, voting_power)| (self.query_public_key(name).unwrap(), *voting_power))
            .collect())
    }

    pub fn get_governance_set(&self) -> Result<Vec<(PublicKey, VotingPower)>, String> {
        let mut governance_set = HashMap::new();
        for member in &self.members {
            if let Some(delegatee) = &member.governance_delegatee {
                governance_set
                    .entry(delegatee.clone())
                    .and_modify(|v| *v += member.consensus_voting_power)
                    .or_insert(member.consensus_voting_power);
            } else {
                governance_set
                    .entry(member.name.clone())
                    .and_modify(|v| *v += member.consensus_voting_power)
                    .or_insert(member.consensus_voting_power);
            }
        }
        Ok(governance_set
            .iter()
            .map(|(name, voting_power)| (self.query_public_key(name).unwrap(), *voting_power))
            .collect())
    }

    pub fn apply_delegate(&mut self, tx: &TxDelegate) -> Result<Self, String> {
        let mut delegator_updated = false;
        let delegator_name = match self.query_name(&tx.delegator.clone()) {
            Some(name) => name,
            None => return Result::Err("delegator does not exist by name".to_string()),
        };
        let delegatee_name = match self.query_name(&tx.delegatee.clone()) {
            Some(name) => name,
            None => return Result::Err("delegatee does not exist by name".to_string()),
        };
        for delegator in &mut self.members {
            if delegator.name == delegator_name {
                if tx.delegatee.to_string().is_empty() {
                    return Result::Err("delegatee field cannot be empty".to_string());
                }
                if tx.governance {
                    delegator.governance_delegatee = Option::from(delegatee_name.clone());
                    delegator.consensus_delegatee = Option::from(delegatee_name.clone());
                    delegator_updated = true;
                } else {
                    delegator.consensus_delegatee = Option::from(delegatee_name.clone());
                    delegator_updated = true;
                }
            }
        }
        if delegator_updated {
            Ok(self.clone())
        } else {
            Err("nothing was delegated. please check your transaction again.".to_string())
        }
    }

    pub fn apply_undelegate(&mut self, tx: &TxUndelegate) -> Result<Self, String> {
        let delegator_name = match self.query_name(&tx.delegator.clone()) {
            Some(name) => name,
            None => return Err("delegator does not exist by name".to_string()),
        };
        let mut delegator_updated = false;
        for delegator in &mut self.members {
            if delegator.name == delegator_name {
                if let Some(_consensus_delegatee) = &delegator.consensus_delegatee {
                    delegator.consensus_delegatee = None;
                    if tx.governance {
                        if let Some(_governance_delegatee) = &delegator.governance_delegatee {
                            delegator.governance_delegatee = None;
                            delegator_updated = true;
                        } else {
                            return Err("governance delegatee is not set".to_string());
                        }
                    } else {
                        delegator_updated = true;
                    }
                } else {
                    return Err("consensus delegatee is not set".to_string());
                }
            }
        }
        if delegator_updated {
            Ok(self.clone())
        } else {
            Err("nothing was undelegated. please check your transaction again.".to_string())
        }
    }

    pub fn query_name(&self, public_key: &PublicKey) -> Option<MemberName> {
        for member in &self.members {
            if &member.public_key == public_key {
                return Some(member.name.clone());
            }
        }
        None
    }

    pub fn query_public_key(&self, name: &MemberName) -> Option<PublicKey> {
        for member in &self.members {
            if &member.name == name {
                return Some(member.public_key.clone());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use simperby_test_suite::setup_test;
    use std::collections::HashSet;

    fn create_member(keys: Vec<(PublicKey, PrivateKey)>, member_num: u8) -> Member {
        Member {
            public_key: keys[member_num as usize].0.clone(),
            name: format!("member-{member_num:04}"),
            governance_voting_power: 1,
            consensus_voting_power: 1,
            governance_delegatee: None,
            consensus_delegatee: None,
        }
    }

    fn create_member_with_consensus_delegation(
        keys: Vec<(PublicKey, PrivateKey)>,
        member_num: u8,
        delegatee_member_num: u8,
    ) -> Member {
        Member {
            public_key: keys[member_num as usize].0.clone(),
            name: format!("member-{member_num:04}"),
            governance_voting_power: 1,
            consensus_voting_power: 1,
            governance_delegatee: None,
            consensus_delegatee: Some(format!("member-{delegatee_member_num:04}")),
        }
    }

    fn create_member_with_governance_delegation(
        keys: Vec<(PublicKey, PrivateKey)>,
        member_num: u8,
        delegatee_member_num: u8,
    ) -> Member {
        Member {
            public_key: keys[member_num as usize].0.clone(),
            name: format!("member-{member_num:04}"),
            governance_voting_power: 1,
            consensus_voting_power: 1,
            governance_delegatee: Some(format!("member-{delegatee_member_num:04}")),
            consensus_delegatee: None,
        }
    }

    #[test]
    fn basic_validator_set1() {
        setup_test();
        let keys = (0..4)
            .into_iter()
            .map(|i| generate_keypair(format!("{i}")))
            .collect::<Vec<_>>();
        let members = vec![
            create_member_with_consensus_delegation(keys.clone(), 0, 3),
            create_member_with_consensus_delegation(keys.clone(), 1, 3),
            create_member_with_consensus_delegation(keys.clone(), 2, 3),
            create_member(keys.clone(), 3),
        ];
        let genesis_header = BlockHeader {
            author: PublicKey::zero(),
            prev_block_finalization_proof: Vec::new(),
            previous_hash: Hash256::zero(),
            height: 0,
            timestamp: 0,
            commit_merkle_root: Hash256::zero(),
            repository_merkle_root: Hash256::zero(),
            validator_set: members
                .iter()
                .map(|member| (member.public_key.clone(), member.consensus_voting_power))
                .collect::<Vec<_>>(),
            version: "0.1.0".to_string(),
        };
        let genesis_info = GenesisInfo {
            header: genesis_header.clone(),
            genesis_proof: keys
                .iter()
                .map(|(_, private_key)| TypedSignature::sign(&genesis_header, private_key).unwrap())
                .collect::<Vec<_>>(),
            chain_name: "test-chain".to_string(),
        };
        let reserved_state = ReservedState {
            genesis_info,
            members,
            consensus_leader_order: vec!["member-0003".to_string()],
            version: "0.1.0".to_string(),
        };
        assert_eq!(
            reserved_state.get_validator_set().unwrap(),
            vec![(keys[3].0.clone(), 4),]
        );
    }

    #[test]
    fn basic_validator_set2() {
        setup_test();
        let keys = (0..4)
            .into_iter()
            .map(|i| generate_keypair(format!("{i}")))
            .collect::<Vec<_>>();
        let members = vec![
            create_member_with_consensus_delegation(keys.clone(), 0, 1),
            create_member(keys.clone(), 1),
            create_member_with_consensus_delegation(keys.clone(), 2, 3),
            create_member(keys.clone(), 3),
        ];
        let genesis_header = BlockHeader {
            author: PublicKey::zero(),
            prev_block_finalization_proof: Vec::new(),
            previous_hash: Hash256::zero(),
            height: 0,
            timestamp: 0,
            commit_merkle_root: Hash256::zero(),
            repository_merkle_root: Hash256::zero(),
            validator_set: members
                .iter()
                .map(|member| (member.public_key.clone(), member.consensus_voting_power))
                .collect::<Vec<_>>(),
            version: "0.1.0".to_string(),
        };
        let genesis_info = GenesisInfo {
            header: genesis_header.clone(),
            genesis_proof: keys
                .iter()
                .map(|(_, private_key)| TypedSignature::sign(&genesis_header, private_key).unwrap())
                .collect::<Vec<_>>(),
            chain_name: "test-chain".to_string(),
        };
        let reserved_state = ReservedState {
            genesis_info,
            members,
            consensus_leader_order: vec!["member-0001".to_string(), "member-0003".to_string()],
            version: "0.1.0".to_string(),
        };
        assert_eq!(
            reserved_state.get_validator_set().unwrap(),
            vec![(keys[1].0.clone(), 2), (keys[3].0.clone(), 2),]
        );
    }

    #[test]
    fn basic_governance_set1() {
        setup_test();
        let keys = (0..4)
            .into_iter()
            .map(|i| generate_keypair(format!("{i}")))
            .collect::<Vec<_>>();
        let members = vec![
            create_member_with_governance_delegation(keys.clone(), 0, 3),
            create_member_with_governance_delegation(keys.clone(), 1, 3),
            create_member_with_governance_delegation(keys.clone(), 2, 3),
            create_member(keys.clone(), 3),
        ];
        let genesis_header = BlockHeader {
            author: PublicKey::zero(),
            prev_block_finalization_proof: Vec::new(),
            previous_hash: Hash256::zero(),
            height: 0,
            timestamp: 0,
            commit_merkle_root: Hash256::zero(),
            repository_merkle_root: Hash256::zero(),
            validator_set: members
                .iter()
                .map(|member| (member.public_key.clone(), member.consensus_voting_power))
                .collect::<Vec<_>>(),
            version: "0.1.0".to_string(),
        };
        let genesis_info = GenesisInfo {
            header: genesis_header.clone(),
            genesis_proof: keys
                .iter()
                .map(|(_, private_key)| TypedSignature::sign(&genesis_header, private_key).unwrap())
                .collect::<Vec<_>>(),
            chain_name: "test-chain".to_string(),
        };
        let reserved_state = ReservedState {
            genesis_info,
            members,
            consensus_leader_order: (0..4)
                .into_iter()
                .map(|i| format!("member-{i:04}"))
                .collect::<Vec<_>>(),
            version: "0.1.0".to_string(),
        };
        assert_eq!(
            reserved_state.get_governance_set().unwrap(),
            vec![(keys[3].0.clone(), 4),]
        );
    }

    #[test]
    fn basic_governance_set2() {
        setup_test();
        let keys = (0..4)
            .into_iter()
            .map(|i| generate_keypair(format!("{i}")))
            .collect::<Vec<_>>();
        let members = vec![
            create_member_with_governance_delegation(keys.clone(), 0, 1),
            create_member(keys.clone(), 1),
            create_member_with_governance_delegation(keys.clone(), 2, 3),
            create_member(keys.clone(), 3),
        ];
        let genesis_header = BlockHeader {
            author: PublicKey::zero(),
            prev_block_finalization_proof: Vec::new(),
            previous_hash: Hash256::zero(),
            height: 0,
            timestamp: 0,
            commit_merkle_root: Hash256::zero(),
            repository_merkle_root: Hash256::zero(),
            validator_set: members
                .iter()
                .map(|member| (member.public_key.clone(), member.consensus_voting_power))
                .collect::<Vec<_>>(),
            version: "0.1.0".to_string(),
        };
        let genesis_info = GenesisInfo {
            header: genesis_header.clone(),
            genesis_proof: keys
                .iter()
                .map(|(_, private_key)| TypedSignature::sign(&genesis_header, private_key).unwrap())
                .collect::<Vec<_>>(),
            chain_name: "test-chain".to_string(),
        };
        let reserved_state = ReservedState {
            genesis_info,
            members,
            consensus_leader_order: (0..4)
                .into_iter()
                .map(|i| format!("member-{i:04}"))
                .collect::<Vec<_>>(),
            version: "0.1.0".to_string(),
        };
        assert_eq!(
            reserved_state
                .get_governance_set()
                .unwrap()
                .into_iter()
                .collect::<HashSet<_>>(),
            vec![(keys[1].0.clone(), 2), (keys[3].0.clone(), 2),]
                .into_iter()
                .collect::<HashSet<_>>()
        );
    }

    fn setup_tx_delegate_test() -> (PublicKey, PrivateKey, PublicKey, Member, ReservedState) {
        let keys = (0..4)
            .into_iter()
            .map(|i| generate_keypair(format!("{i}")))
            .collect::<Vec<_>>();

        let delegator_public_key = keys[0].0.clone();
        let delegator_private_key = keys[0].1.clone();
        let delegatee_public_key = keys[1].0.clone();

        let delegator = Member {
            public_key: delegator_public_key.clone(),
            name: "delegator".to_string(),
            consensus_voting_power: 10,
            governance_voting_power: 10,
            consensus_delegatee: None,
            governance_delegatee: None,
        };

        let delegatee = Member {
            public_key: delegatee_public_key.clone(),
            name: "delegatee".to_string(),
            consensus_voting_power: 20,
            governance_voting_power: 20,
            consensus_delegatee: None,
            governance_delegatee: None,
        };

        let members = vec![
            delegator.clone(),
            delegatee.clone(),
            create_member(keys.clone(), 2),
        ];

        let genesis_header = BlockHeader {
            author: PublicKey::zero(),
            prev_block_finalization_proof: Vec::new(),
            previous_hash: Hash256::zero(),
            height: 0,
            timestamp: 0,
            commit_merkle_root: Hash256::zero(),
            repository_merkle_root: Hash256::zero(),
            validator_set: members
                .iter()
                .map(|member| (member.public_key.clone(), member.consensus_voting_power))
                .collect::<Vec<_>>(),
            version: "0.1.0".to_string(),
        };

        let genesis_info = GenesisInfo {
            header: genesis_header.clone(),
            genesis_proof: keys
                .iter()
                .map(|(_, private_key)| TypedSignature::sign(&genesis_header, private_key).unwrap())
                .collect::<Vec<_>>(),
            chain_name: "test-chain".to_string(),
        };

        let state = ReservedState {
            genesis_info,
            members: vec![delegator.clone(), delegatee.clone()],
            consensus_leader_order: vec![delegator.name, delegatee.name.to_string()],
            version: "".to_string(),
        };
        (
            delegator_public_key,
            delegator_private_key,
            delegatee_public_key,
            delegatee,
            state,
        )
    }

    #[test]
    fn test_apply_delegate_on_governance_success() {
        // given
        setup_test();
        let (
            delegator_public_key,
            delegator_private_key,
            delegatee_public_key,
            delegatee,
            mut state,
        ) = setup_tx_delegate_test();

        // when
        let data = (
            delegator_public_key.clone(),
            delegatee_public_key.clone(),
            true,
            0u64,
        );
        let proof = TypedSignature::sign(&data, &delegator_private_key).unwrap();

        let tx = TxDelegate {
            delegator: delegator_public_key,
            delegatee: delegatee_public_key,
            governance: true,
            proof,
            timestamp: 0,
        };
        let new_state = state.apply_delegate(&tx);

        // then
        assert!(new_state.is_ok());
        let new_state = new_state.unwrap();
        let new_state_validator_set = new_state.get_validator_set();
        let new_state_governance_set = new_state.get_governance_set();

        assert_eq!(
            new_state.members[0].governance_delegatee.as_ref().unwrap(),
            &delegatee.name
        );
        assert_eq!(new_state.members[0].consensus_voting_power, 10);
        assert_eq!(new_state.members[0].governance_voting_power, 10);
        assert_eq!(
            new_state.members[0].consensus_delegatee,
            Some(delegatee.clone().name)
        );
        assert_eq!(
            new_state.members[0].governance_delegatee,
            Some(delegatee.clone().name)
        );
        assert_eq!(new_state.members[1].consensus_voting_power, 20);
        assert_eq!(new_state.members[1].governance_voting_power, 20);

        assert_eq!(
            new_state_validator_set
                .unwrap()
                .into_iter()
                .find(|v| v.0 == delegatee.public_key)
                .unwrap()
                .1,
            30
        );

        assert_eq!(
            new_state_governance_set
                .unwrap()
                .into_iter()
                .find(|v| v.0 == delegatee.public_key)
                .unwrap()
                .1,
            30
        );
    }

    #[test]
    fn test_apply_delegate_on_consensus_success() {
        // given
        setup_test();
        let (
            delegator_public_key,
            delegator_private_key,
            delegatee_public_key,
            delegatee,
            mut state,
        ) = setup_tx_delegate_test();

        // when
        let data = (
            delegator_public_key.clone(),
            delegatee_public_key.clone(),
            true,
            0u64,
        );
        let proof = TypedSignature::sign(&data, &delegator_private_key).unwrap();

        let tx = TxDelegate {
            delegator: delegator_public_key,
            delegatee: delegatee_public_key,
            governance: false,
            proof,
            timestamp: 0,
        };

        let new_state = state.apply_delegate(&tx);

        // then
        assert!(new_state.is_ok());
        let new_state = new_state.unwrap();
        let new_state_validator_set = new_state.get_validator_set();
        let new_state_governance_set = new_state.get_governance_set();

        assert_eq!(
            new_state.members[0].consensus_delegatee.as_ref().unwrap(),
            &delegatee.name
        );
        assert_eq!(new_state.members[0].consensus_voting_power, 10);
        assert_eq!(new_state.members[0].governance_voting_power, 10);
        assert_eq!(
            new_state.members[0].consensus_delegatee,
            Some(delegatee.name)
        );
        assert_eq!(new_state.members[0].governance_delegatee, None);
        assert_eq!(new_state.members[1].consensus_voting_power, 20);
        assert_eq!(new_state.members[1].governance_voting_power, 20);

        assert_eq!(
            new_state_validator_set
                .unwrap()
                .into_iter()
                .find(|v| v.0 == delegatee.public_key)
                .unwrap()
                .1,
            30
        );

        assert_eq!(
            new_state_governance_set
                .unwrap()
                .into_iter()
                .find(|v| v.0 == delegatee.public_key)
                .unwrap()
                .1,
            20
        );
    }

    fn setup_tx_undelegate_on_governance_test(
    ) -> (PublicKey, PrivateKey, PublicKey, Member, ReservedState) {
        let keys = (0..4)
            .into_iter()
            .map(|i| generate_keypair(format!("{i}")))
            .collect::<Vec<_>>();

        let delegator_public_key = keys[0].0.clone();
        let delegator_private_key = keys[0].1.clone();
        let delegatee_public_key = keys[1].0.clone();

        let delegator = Member {
            public_key: delegator_public_key.clone(),
            name: "delegator".to_string(),
            consensus_voting_power: 10,
            governance_voting_power: 10, // delegated
            consensus_delegatee: Some("delegatee".to_string()),
            governance_delegatee: Some("delegatee".to_string()),
        };

        let delegatee = Member {
            public_key: delegatee_public_key.clone(),
            name: "delegatee".to_string(),
            consensus_voting_power: 20,
            governance_voting_power: 20,
            consensus_delegatee: None,
            governance_delegatee: None,
        };

        let members = vec![
            delegator.clone(),
            delegatee.clone(),
            create_member(keys.clone(), 2),
        ];

        let genesis_header = BlockHeader {
            author: PublicKey::zero(),
            prev_block_finalization_proof: Vec::new(),
            previous_hash: Hash256::zero(),
            height: 0,
            timestamp: 0,
            commit_merkle_root: Hash256::zero(),
            repository_merkle_root: Hash256::zero(),
            validator_set: members
                .iter()
                .map(|member| (member.public_key.clone(), member.consensus_voting_power))
                .collect::<Vec<_>>(),
            version: "0.1.0".to_string(),
        };

        let genesis_info = GenesisInfo {
            header: genesis_header.clone(),
            genesis_proof: keys
                .iter()
                .map(|(_, private_key)| TypedSignature::sign(&genesis_header, private_key).unwrap())
                .collect::<Vec<_>>(),
            chain_name: "test-chain".to_string(),
        };

        let state = ReservedState {
            genesis_info,
            members: vec![delegator.clone(), delegatee.clone()],
            consensus_leader_order: vec![delegator.name, delegatee.name.to_string()],
            version: "".to_string(),
        };
        (
            delegator_public_key,
            delegator_private_key,
            delegatee_public_key,
            delegatee,
            state,
        )
    }

    #[test]
    fn test_apply_undelegate_on_governance_success() {
        // given
        setup_test();
        let (
            delegator_public_key,
            delegator_private_key,
            delegatee_public_key,
            delegatee,
            mut state,
        ) = setup_tx_undelegate_on_governance_test();

        // when
        let data = (
            delegator_public_key.clone(),
            delegatee_public_key.clone(),
            true,
            0u64,
        );
        let proof = TypedSignature::sign(&data, &delegator_private_key).unwrap();

        let tx = TxUndelegate {
            delegator: delegator_public_key,
            delegatee: delegatee_public_key,
            governance: true,
            proof,
            timestamp: 0,
        };
        let new_state = state.apply_undelegate(&tx);

        // then
        assert!(new_state.is_ok());
        let new_state = new_state.unwrap();
        let new_state_validator_set = new_state.get_validator_set();
        let new_state_governance_set = new_state.get_governance_set();

        assert_eq!(new_state.members[0].consensus_voting_power, 10);
        assert_eq!(new_state.members[0].governance_voting_power, 10);
        assert_eq!(new_state.members[0].consensus_delegatee, None);
        assert_eq!(new_state.members[0].governance_delegatee, None);
        assert_eq!(new_state.members[1].consensus_voting_power, 20);
        assert_eq!(new_state.members[1].governance_voting_power, 20);

        assert_eq!(
            new_state_validator_set
                .unwrap()
                .into_iter()
                .find(|v| v.0 == delegatee.public_key)
                .unwrap()
                .1,
            20
        );

        assert_eq!(
            new_state_governance_set
                .unwrap()
                .into_iter()
                .find(|v| v.0 == delegatee.public_key)
                .unwrap()
                .1,
            20
        );
    }

    fn setup_tx_undelegate_on_consensus_test(
    ) -> (PublicKey, PrivateKey, PublicKey, Member, ReservedState) {
        let keys = (0..4)
            .into_iter()
            .map(|i| generate_keypair(format!("{i}")))
            .collect::<Vec<_>>();

        let delegator_public_key = keys[0].0.clone();
        let delegator_private_key = keys[0].1.clone();
        let delegatee_public_key = keys[1].0.clone();

        let delegator = Member {
            public_key: delegator_public_key.clone(),
            name: "delegator".to_string(),
            consensus_voting_power: 10, // delegated
            governance_voting_power: 10,
            consensus_delegatee: Some("delegatee".to_string()),
            governance_delegatee: None,
        };

        let delegatee = Member {
            public_key: delegatee_public_key.clone(),
            name: "delegatee".to_string(),
            consensus_voting_power: 10,
            governance_voting_power: 10,
            consensus_delegatee: None,
            governance_delegatee: None,
        };

        let members = vec![
            delegator.clone(),
            delegatee.clone(),
            create_member(keys.clone(), 2),
        ];

        let genesis_header = BlockHeader {
            author: PublicKey::zero(),
            prev_block_finalization_proof: Vec::new(),
            previous_hash: Hash256::zero(),
            height: 0,
            timestamp: 0,
            commit_merkle_root: Hash256::zero(),
            repository_merkle_root: Hash256::zero(),
            validator_set: members
                .iter()
                .map(|member| (member.public_key.clone(), member.consensus_voting_power))
                .collect::<Vec<_>>(),
            version: "0.1.0".to_string(),
        };

        let genesis_info = GenesisInfo {
            header: genesis_header.clone(),
            genesis_proof: keys
                .iter()
                .map(|(_, private_key)| TypedSignature::sign(&genesis_header, private_key).unwrap())
                .collect::<Vec<_>>(),
            chain_name: "test-chain".to_string(),
        };

        let state = ReservedState {
            genesis_info,
            members: vec![delegator.clone(), delegatee.clone()],
            consensus_leader_order: vec![delegator.name, delegatee.name.to_string()],
            version: "".to_string(),
        };
        (
            delegator_public_key,
            delegator_private_key,
            delegatee_public_key,
            delegatee,
            state,
        )
    }

    #[test]
    fn test_apply_undelegate_on_consensus_success() {
        // given
        setup_test();
        let (
            delegator_public_key,
            delegator_private_key,
            delegatee_public_key,
            delegatee,
            mut state,
        ) = setup_tx_undelegate_on_consensus_test();

        // when
        let data = (
            delegator_public_key.clone(),
            delegatee_public_key.clone(),
            true,
            0u64,
        );
        let proof = TypedSignature::sign(&data, &delegator_private_key).unwrap();

        let tx = TxUndelegate {
            delegator: delegator_public_key,
            delegatee: delegatee_public_key,
            governance: false,
            proof,
            timestamp: 0,
        };
        let new_state = state.apply_undelegate(&tx);

        // then
        assert!(new_state.is_ok());
        let new_state = new_state.unwrap();
        let new_state_validator_set = new_state.get_validator_set();
        let new_state_governance_set = new_state.get_governance_set();

        assert_eq!(new_state.members[0].governance_delegatee, None);
        assert_eq!(new_state.members[0].consensus_delegatee, None);
        assert_eq!(new_state.members[0].consensus_voting_power, 10);
        assert_eq!(new_state.members[0].governance_voting_power, 10);
        assert_eq!(new_state.members[1].consensus_voting_power, 10);
        assert_eq!(new_state.members[1].governance_voting_power, 10);

        assert_eq!(
            new_state_validator_set
                .unwrap()
                .into_iter()
                .find(|v| v.0 == delegatee.public_key)
                .unwrap()
                .1,
            10
        );

        assert_eq!(
            new_state_governance_set
                .unwrap()
                .into_iter()
                .find(|v| v.0 == delegatee.public_key)
                .unwrap()
                .1,
            10
        );
    }
}
