# Claudei Containers

In the below reference, user has created a dockerfile which is already build and image name is workspace-claude-cli:latest.

```bash
ls reference/docker-workspace
```

- limitation is that it is mounted to a specific project.
- requires docker and docker eco systems.

## Overview

- Need to have a containerized workspace for claude cli
- need to learn linux kernel what gets used under the hood.
- create a prototype to run with bash first using docker. Then plan to replace with kernal libraries with rust.
- host project directory bind-mounted at `/workspace` inside the container's mount namespace

## How user imagines it works

- provided the dockerfile exists and image is built

```bash

cd ~/project/
claudei

```

a container is running with project as working directory.

```bash

cd ~/project/
claudei replace

```

If the continer is already running then **replace** should just switch the bind-mounted at `/workspace`
a container is running with project as working directory.

### Requirements

- Accept a project path as argument (defaults to $PWD)
- Mount that path to /workspace inside the container
- Keep the security hardening (read-only, tmpfs, resource limits, no-new-privileges)
- Named volume `claude-home` for /home/claude-user persistence across runs
- No project-specific deps (no db network, no env_file)

## Contents

- [docker-kernel-mapping.md](docker-kernel-mapping.md) — Docker flags to kernel syscalls, Dockerfile/compose parsed, inspect commands
- [container-rust-phases.md](container-rust-phases.md) — Phases 0-7: replacing Docker with direct kernel calls in Rust


# spacecode
