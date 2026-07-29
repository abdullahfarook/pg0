# Changelog

All notable changes to pg0 are documented in this file.

## [0.15.0] - 2026-07-29

### Added

- Active database health checks for `pg0 info`, `pg0 list`, and SDK status reporting.

### Changed

- Upgraded bundled pgvector to 0.8.5.
- Rebuilt Linux pgvector artifacts against a GLIBC 2.35 baseline for compatibility with supported GNU/Linux hosts.
- Removed the Node.js SDK and npm release support.

### Fixed

- Windows startup for existing instances no longer depends on the blocking `tasklist` command.

## [0.14.2] - 2026-05-28

### Fixed

- Wait for PostgreSQL to fully shut down before returning from `pg0 stop`.

## [0.14.1] - 2026-05-08

### Fixed

- Make existing-instance startup independent of localized duplicate-database error messages.

## [0.14.0] - 2026-05-05

### Changed

- Bundle libxml2 and ICU runtime libraries for broader Linux compatibility.

## [0.13.0] - 2026-04-30

### Changed

- Synchronize the committed Cargo lockfile as part of releases for reproducible builds.
