//! `std.agent.protocol`: expose a Spectra agent over other agent protocols
//! (R-3219).
//!
//! Interop is an adapter, not an architecture. Both adapters here are built on
//! the primitives that already exist — a run, its journal, and the approval
//! primitive — and neither introduces a second dispatch path: every effect an
//! adapter performs reaches compiled tools through the governed dispatch
//! ([`crate::act::act`] / [`crate::act::tool_call`]), so the capability grant,
//! the tool-call ceiling, the journal step and the taint gate apply exactly as
//! they do to a local `tool_call`.
//!
//! * [`a2a`] (T1): the A2A agent card built from the *derived* tool surface,
//!   and a task lifecycle whose tasks are journaled runs — a task id maps to a
//!   run id, so a client can poll a task and a restart resumes it instead of
//!   re-delegating it.
//! * [`acp`] (T2): the ACP agent surface and its permission bridge, where the
//!   agent's `session/request_permission` is answered by the attached client
//!   and the answer is journaled by [`crate::approval`] as an
//!   allow-once/allow-always/deny decision.
//!
//! # Transport
//!
//! Both protocols speak JSON-RPC 2.0, so each adapter is a **pure per-request
//! handler** ([`a2a::handle`], [`acp::handle`]) a host stack can route to, plus
//! one outbound seam the embedding application owns:
//!
//! * A2A's inbound path is [`a2a::serve`], which carries the crate's shared
//!   in-crate listener ([`crate::net`]) so a Spectra project is reachable by a
//!   third-party client today. It is the only place this module opens a
//!   socket; an application with its own server routes [`a2a::handle`] instead.
//! * ACP's client side is [`acp::AcpClient`], installed with
//!   [`acp::set_acp_client`]. The application owns the pipe (stdio, a socket,
//!   a UI); with none attached the bridge denies, so an unattended run never
//!   authorizes an action.

pub(crate) mod a2a;
pub(crate) mod acp;
