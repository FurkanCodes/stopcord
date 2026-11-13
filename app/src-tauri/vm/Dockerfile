# Stopcord's VM image: Linux with git, node, python and the harnesses Stopcord knows.
# Your own image for a runner should start FROM stopcord-vm:latest, so st and the sync stay.
FROM node:22-bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends git ca-certificates socat python3 curl procps less openssh-client \
    && rm -rf /var/lib/apt/lists/*

RUN npm install -g @anthropic-ai/claude-code @openai/codex opencode-ai @google/gemini-cli \
    && npm cache clean --force

# The node image's user, renamed. Agents never run as root.
RUN usermod -l agent -d /home/agent -m node && groupmod -n agent node \
    && mkdir -p /work && chown agent:agent /work

COPY --chmod=755 st stopcord-sync relay probe /usr/local/bin/

USER agent
ENV HOME=/home/agent
WORKDIR /work
