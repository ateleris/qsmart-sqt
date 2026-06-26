# Minimal run image for sqt-runner's "run" tool.
#
# Based on Ubuntu 24.04 (glibc 2.39) so it can execute a host-built Rust binary,
# plus GNU time so the runner can capture peak RSS and CPU%.
# run_binary.rs uses `/usr/bin/time -v` when present and falls back to plain
# execution (RSS/CPU = null) when it is not.
#
# The binary-under-test is bind-mounted at run time by sqt-runner — nothing is
# COPYed into this image.
FROM ubuntu:24.04

RUN apt-get update \
 && apt-get install -y --no-install-recommends time \
 && rm -rf /var/lib/apt/lists/*

CMD ["/bin/sh"]
