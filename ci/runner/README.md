# Self-hosted runner (Docker-in-Docker)

A containerized GitHub Actions runner for **ats-sqt**, modeled on GitLab's
docker executor: a thin runner agent + an isolated Docker-in-Docker (dind)
daemon. Jobs don't run inside the runner — they launch the `ci` build image
via `docker run` against dind, so each job gets a fresh toolchain container
while the runner itself stays minimal.

## Architecture

```
┌─ runner (myoung34/github-runner) ──┐      ┌─ dind (docker:27-dind) ─────────┐
│  registers to the repo             │ TLS  │  isolated Docker daemon         │
│  docker CLI ─────────────────────────────▶│  runs the ci build image        │
│  DOCKER_HOST=tcp://dind:2376       │ 2376 │  /var/lib/docker = image cache  │
└────────────────────────────────────┘      └─────────────────────────────────┘
        shared volume: /actions-runner/_work (the job checkout)
```

The toolchain lives in `../Dockerfile` (the build image). The workflow builds
it on the dind daemon on demand (`docker build - < ci/Dockerfile`) and caches
it in dind's persistent storage — no registry, no push, no pull. The runner
image is the stock `myoung34/github-runner` — nothing custom to build.

## Deploy

```sh
cp .env.example .env
# edit .env: REPO_URL + ACCESS_TOKEN
docker compose up -d
docker compose logs -f runner   # confirm "Listening for Jobs"
```

The runner appears under **Repo → Settings → Actions → Runners**. It runs
ephemerally (one job per registration) and `restart: always` re-registers it
after each job and across reboots (enable the Docker service: `systemctl
enable docker`).

## Why `docker run`, not the `container:` job key

GitHub's native `container:` key is the obvious GitLab `image:` analog, but
over a *remote* dind daemon it's fragile: the runner mirrors both its `_work`
**and** `externals` (bundled node) directories into the job container by path,
which only resolves if those volumes are replicated into dind at identical
paths. Instead, `rust.yml` runs the build image explicitly:

```sh
docker run --rm -v "$GITHUB_WORKSPACE":"$GITHUB_WORKSPACE" -w "$GITHUB_WORKSPACE" \
  ats-sqt-ci:latest bash ci/run-tests.sh
```

This is what GitLab's docker executor does under the hood, and it only needs
the **workspace** shared with dind (the `runner-work` volume) — no externals
gymnastics. JS actions like `actions/checkout` and `dorny/test-reporter` still
run on the runner via its bundled node.

> To switch to the lighter Docker-out-of-Docker model later: drop the `dind`
> service, bind-mount `/var/run/docker.sock` into the runner, and remove the
> `DOCKER_HOST`/TLS env. The host daemon then runs the build containers and
> shares its image cache.

## Notes & caveats

- **Privileged.** dind requires `--privileged`, giving that container broad
  host access. Fine for infra you own; don't expose this runner to untrusted
  PRs from forks.
- **PAT scope.** `ACCESS_TOKEN` is a repo-admin-capable token. Store `.env`
  with tight permissions; rotate if leaked. A GitHub App is the least-privilege
  upgrade if this ever becomes shared infra.
- **First image build.** No pre-seeding needed — the first job builds
  `ats-sqt-ci:latest` on dind (a few minutes for the toolchain layers), and
  every later job is a cache hit until `ci/Dockerfile` changes.
- **Validation.** The dind + shared-workspace wiring should be smoke-tested on
  the actual host (push a commit, watch `docker compose logs -f`). The path
  match between the two `runner-work` mounts is the load-bearing detail.
