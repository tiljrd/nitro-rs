# nitro-rs

A full Arbitrum Nitro node implemented in Rust, using:
- Execution client: reth (tiljrd/reth) with dedicated Arbitrum crates
- Arbitrum primitives: arb-alloy (tiljrd/arb-alloy)
- Architecture and behavior mirroring the reference Go implementation (tiljrd/nitro) exactly

Goals
- Exact parity with Nitro for consensus/storage encodings, transaction envelopes (2718 typed), and receipt formats. In particular, consensus RLP must not include L1 gas fields.
- Full subsystem coverage without mocks: InboxReader/Tracker, TransactionStreamer, BatchPoster, Validator interfaces (BOLD pathway), Data Availability (DAS) integration points, and RPC (NodeInterface semantics).
- Modularity: no forking of base reth; Arbitrum-specific logic lives in separate crates and a dedicated binary.

Workspace Layout
- crates/primitives: Nitro-specific DB keys/encodings and helper types where needed for parity.
- crates/inbox: InboxTracker and accumulator/state tracking (batches and delayed), reorg support.
- crates/inbox-reader: L1 reader integration and run loop (latest/safe/finalized, delay-blocks, dynamic blocksToFetch, reorg detection).
- crates/streamer: TransactionStreamer implementing message ingestion, reorg handling, persistence, and broadcasting semantics.
- crates/batch-poster: Batch building (brotli), 4844 blobs, L1 bounds checks, gas estimation, DA writer integration, Redis lock.
- crates/validator: Validator interfaces for block validation and BOLD pathway hooks.
- crates/rpc: NodeInterface implementations and Arbitrum-specific RPC surfaces.
- crates/node: Node orchestrator (config, wiring to reth Arb EVM and Engine API), lifecycle, metrics.
- bin/arb-nitro-rs: CLI binary that starts a full node.

Execution Integration
- Uses reth’s ArbEvmConfig and ArbEngine API wrapper for Arbitrum (arb-reth components).
- Receipt handling via reth_arbitrum_evm::ArbRethReceiptBuilder and primitives in reth_arbitrum_primitives.
- Predeploy registry and retryables wired to reth’s arbitrum EVM.

Docker
- A Dockerfile is provided to build a runnable image expected by tiljrd/arbitrum-package.
- See section “Running with arbitrum-package” for orchestration.

Status
- Initial workspace scaffolding.
- Next steps: implement InboxTracker, InboxReader, and TransactionStreamer with exact Nitro parity.

Contributing
- Many small commits with clear messages; one PR per repository.
- No mocks or placeholders for core functionality.

License
MIT OR Apache-2.0
