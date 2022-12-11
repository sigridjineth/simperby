use super::*;
use eyre::eyre;
use simperby_consensus::{Consensus, ConsensusParameters};
use simperby_network::primitives::{GossipNetwork, Storage};
use simperby_network::NetworkConfig;
use simperby_network::{dms, storage::StorageImpl, Dms, Peer, SharedKnownPeers};
use simperby_repository::raw::{RawRepository, RawRepositoryImpl};
use simperby_repository::DistributedRepository;

pub struct Node<N: GossipNetwork, S: Storage, R: RawRepository> {
    config: Config,
    repository: DistributedRepository<R>,
    governance: Governance<N, S>,
    consensus: Consensus<N, S>,

    last_reserved_state: ReservedState,
    #[allow(dead_code)]
    last_finalized_header: BlockHeader,
}

impl SimperbyNode {
    pub async fn initialize(config: Config, path: &str) -> Result<Self> {
        // Step 0: initialize the repository module
        let peers: Vec<Peer> = serde_json::from_str(
            &tokio::fs::read_to_string(&format!("{}/peers.json", path)).await?,
        )?;
        let peers = SharedKnownPeers::new_static(peers.clone());
        let raw_repository = RawRepositoryImpl::open(&format!("{}/repository/repo", path)).await?;
        let repository = DistributedRepository::new(
            raw_repository,
            simperby_repository::Config {
                mirrors: config.public_repo_url.clone(),
                long_range_attack_distance: 3,
            },
            peers.clone(),
        )
        .await?;

        // Step 1: initialize configs
        let last_finalized_header = repository.get_last_finalized_block_header().await?;
        let reserved_state = repository.get_reserved_state().await?;
        let governance_dms_key = simperby_governance::generate_dms_key(&last_finalized_header);
        let consensus_dms_key = simperby_consensus::generate_dms_key(&last_finalized_header);
        let network_config = NetworkConfig {
            network_id: reserved_state.genesis_info.chain_name.clone(),
            ports: vec![
                (governance_dms_key.clone(), 1555),
                (consensus_dms_key.clone(), 1556),
            ]
            .into_iter()
            .collect(),
            members: reserved_state
                .members
                .iter()
                .map(|m| m.public_key.clone())
                .collect(),
            public_key: config.public_key.clone(),
            private_key: config.private_key.clone(),
        };
        let dms_config = dms::Config {
            fetch_interval: Some(std::time::Duration::from_millis(500)),
            broadcast_interval: Some(std::time::Duration::from_millis(500)),
            network_config,
        };

        // Step 2: initialize the governance module
        StorageImpl::create(&format!("{}/governance/dms", path))
            .await
            .unwrap();
        let storage = StorageImpl::open(path).await.unwrap();
        let dms = Dms::new(
            storage,
            governance_dms_key,
            dms_config.clone(),
            peers.clone(),
        )
        .await?;
        let governance = Governance::new(dms, Some(config.private_key.clone())).await?;

        // Step 3: initialize the consensus module
        StorageImpl::create(&format!("{}/consensus/dms", path))
            .await
            .unwrap();
        let storage = StorageImpl::open(path).await.unwrap();
        let dms = Dms::new(
            storage,
            consensus_dms_key,
            dms_config.clone(),
            peers.clone(),
        )
        .await?;
        StorageImpl::create(&format!("{}/consensus/state", path))
            .await
            .unwrap();
        let consensus_state_storage = StorageImpl::open(path).await.unwrap();
        let consensus = Consensus::new(
            dms,
            consensus_state_storage,
            last_finalized_header.clone(),
            // TODO: replace params and timestamp with proper values
            ConsensusParameters {
                timeout_ms: 0,
                repeat_round_for_first_leader: 0,
            },
            0,
            Some(config.private_key.clone()),
        )
        .await?;
        Ok(Self {
            config,
            repository,
            governance,
            consensus,
            last_reserved_state: reserved_state,
            last_finalized_header,
        })
    }

    pub fn get_raw_repo(&self) -> &impl RawRepository {
        self.repository.get_raw()
    }

    pub fn get_raw_repo_mut(&mut self) -> &mut impl RawRepository {
        self.repository.get_raw_mut()
    }
}

fn get_timestamp() -> Timestamp {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as Timestamp
}

#[async_trait]
impl<N: GossipNetwork, S: Storage, R: RawRepository> SimperbyApi for Node<N, S, R> {
    async fn genesis(&mut self) -> Result<()> {
        todo!()
    }

    async fn sync(&mut self, _commmit: CommitHash) -> Result<()> {
        todo!()
    }

    async fn clean(&mut self, _hard: bool) -> Result<()> {
        self.repository.clean().await
    }

    async fn create_block(&mut self) -> Result<CommitHash> {
        let (header, commit_hash) = self
            .repository
            .create_block(self.config.public_key.clone())
            .await?;
        // automatically set as my proposal
        self.consensus
            .register_verified_block_hash(header.to_hash256())
            .await?;
        self.consensus
            .set_proposal_candidate(header.to_hash256(), get_timestamp())
            .await?;
        Ok(commit_hash)
    }

    async fn create_agenda(&mut self) -> Result<CommitHash> {
        let (_, commit_hash) = self
            .repository
            .create_agenda(self.config.public_key.clone())
            .await?;
        Ok(commit_hash)
    }

    async fn create_extra_agenda_transaction(&mut self, _tx: ExtraAgendaTransaction) -> Result<()> {
        unimplemented!()
    }

    async fn vote(&mut self, agenda_commit: CommitHash) -> Result<()> {
        let valid_agendas = self.repository.get_agendas().await?;
        let agenda_hash = if let Some(x) = valid_agendas.iter().find(|(x, _)| *x == agenda_commit) {
            x.1
        } else {
            return Err(eyre!(
                "the given commit hash {} is not one of the valid agendas",
                agenda_commit
            ));
        };
        self.repository.vote(agenda_commit).await?;
        self.governance.vote(agenda_hash).await?;
        Ok(())
    }

    async fn veto_round(&mut self) -> Result<()> {
        unimplemented!()
    }

    async fn veto_block(&mut self, _block_commit: CommitHash) -> Result<()> {
        unimplemented!()
    }

    async fn show(&self, commit_hash: CommitHash) -> Result<CommitInfo> {
        let semantic_commit = self
            .repository
            .get_raw()
            .read_semantic_commit(commit_hash)
            .await?;
        let commit = simperby_repository::format::from_semantic_commit(semantic_commit.clone())?;
        let result = match commit {
            Commit::Block(block_header) => CommitInfo::Block {
                semantic_commit,
                block_header,
            },
            Commit::Agenda(agenda) => CommitInfo::Agenda {
                semantic_commit,
                agenda: agenda.clone(),
                voters: self
                    .governance
                    .read()
                    .await?
                    .votes
                    .get(&agenda.to_hash256())
                    .unwrap_or(&Default::default())
                    .iter()
                    .filter_map(|public_key| {
                        self.last_reserved_state
                            .query_name(public_key)
                            .map(|x| (x, 0))
                    })
                    .collect(), // TODO
            },
            Commit::AgendaProof(agenda_proof) => CommitInfo::AgendaProof {
                semantic_commit,
                agenda_proof,
            },
            x => CommitInfo::Unknown {
                semantic_commit,
                msg: format!("{:?}", x),
            },
        };
        Ok(result)
    }

    async fn run(self) -> Result<()> {
        unimplemented!()
    }

    async fn progress_for_consensus(&mut self) -> Result<String> {
        let result = self.consensus.progress(get_timestamp()).await?;
        Ok(format!("{:?}", result))
    }

    async fn get_consensus_status(&self) -> Result<ConsensusStatus> {
        todo!()
    }

    async fn get_network_status(&self) -> Result<NetworkStatus> {
        unimplemented!()
    }

    async fn serve(self) -> Result<Self> {
        todo!()
    }

    async fn fetch(&mut self) -> Result<()> {
        let t1 = async { self.governance.fetch().await };
        let t2 = async { self.consensus.fetch().await };
        let t3 = async { self.repository.fetch().await };
        futures::try_join!(t1, t2, t3)?;
        Ok(())
    }
}
