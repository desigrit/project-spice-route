# Product

<!-- impeccable:product-schema 1 -->

## Platform

native Windows desktop

The current Windows interface uses native WinUI 3 with a shared Rust sync engine. A Tauri maintenance build remains in the repository for reference. macOS support is a later objective.

## Users

People who use Codex on more than one personal computer and want to resume their chats and unfinished project work on another device. One device is active at a time.

## Product Purpose

Transfer selected Codex conversations, project organization, and portable workspaces through an installed cloud drive client, with visible review and recoverable restoration.

## Operating Context

Google Drive, OneDrive, or iCloud Drive manages authentication and delivery. Users Push on one device and Pull on the next. Codex must be closed before capture or restoration. Devices may have different local project paths and Codex versions.

## Capabilities and Constraints

- Projectless chats are included by default, with individual exclusions.
- Projects support Full project, Chat history only, and Excluded selections, with per-project local folders.
- Snapshots are immutable; compatibility checks, content verification, ancestry, conflict review, and local rollback protect restoration.
- A successful local publication cannot prove completion of cloud delivery. The app distinguishes visible snapshots from received and verified snapshots.
- The current user reports slow startup and transfer preparation, flashing command windows, oversized captures, unclear warnings, and failed restoration. These are defects to investigate, not capabilities to claim as solved in design examples.
- The user now requests that project files needed to resume work, including secrets, be included. Configuration and messaging must match the implemented policy.
- Design examples use illustrative data and must not be presented as live successful transfers.

## Brand Commitments

Project Spice Route uses the user-selected **C, sweeping sail** icon in flat navy and warm ivory. This brand update preserves the current Workspace interface and transfer behavior. The Windows app should feel native, calm, compact, and clear. Never use em dashes in public content.

## Evidence on Hand

The repository contains the React interface, Rust engine, automated tests, installer artifacts, screenshots, and the selected sweeping-sail icon. The user's September 18 screenshots show the current Overview and cramped review dialog.

## Product Principles

- Make the next safe action obvious.
- Show progress that describes actual work.
- Reserve warnings for information that changes a user's decision.
- Give large file lists enough space and usable grouping.
- Preserve history and recoverability without making ordinary handoff unnecessarily expensive.

## Accessibility and Inclusion

Support keyboard navigation, visible focus, readable text, light and dark themes, and accessible controls.
