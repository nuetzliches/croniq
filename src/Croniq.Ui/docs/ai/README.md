# Croniq UI – AI Tooling Guide

This document captures the actionable pieces from https://next.angular.dev/ai/develop-with-ai so the team can wire AI assistants into this repo quickly.

## 1. Instruction & Rules Files

- VS Code / Windsurf: already configured via [../.instructions.md](../../.instructions.md). Surface this file when prompting Copilot-like tools.
- GitHub Copilot platform: mirror the same content inside `.github/copilot-instructions.md` if you want GitHub.com chat to follow the rules.
- Cursor / JetBrains / Firebase Studio: Angular provides ready-made templates (cursor.md, guidelines.md, airules.md). Pull the latest versions from https://next.angular.dev/assets/context/ when needed and adapt with Croniq-specific guidance.

## 2. Angular CLI MCP Server

- The Angular CLI ships an experimental Model Context Protocol server that exposes project-aware commands (generate components, run tests, etc.).
- Install dependencies: `npm install` (already part of the project setup).
- Start the server from the workspace root:
  - `npm run mcp` (see the VS Code task "Angular MCP Server" for a background process).
- Configure your AI client (e.g., Windsurf, Cursor, Claude Desktop) to connect to the MCP server using the command above as the transport. This grants the assistant safe access to Angular CLI actions described in https://next.angular.dev/ai/mcp.
- VS Code / Windsurf wiring:
  - Run `Terminal → Run Task → Angular MCP Server` to keep the MCP server alive while you chat with the assistant.
  - Point your AI IDE to the same workspace root so `npm run mcp` resolves the local `node_modules/.bin/ng` shim with project-specific builders.
  - If your tool accepts a command path, set it to `npm` with args `run mcp`; otherwise provide the absolute path to `node_modules/.bin/ng` and pass `mcp --stdio`.

## 3. Web Codegen Scorer

- Repo: https://github.com/angular/web-codegen-scorer.
- Use this tool to benchmark AI-generated UI snippets before committing them. Feed reference designs plus generated code to obtain a quantitative score; iterate until it meets your team threshold.

## 4. llms.txt Context Files

- Angular publishes `llms.txt` and `llms-full.txt` describing modern framework guidance.
- Host these files if you deploy public docs for Croniq UI so AI web crawlers can ingest accurate guidance. Start from https://next.angular.dev/llms.txt and tailor the links to Croniq-specific docs.

## 5. Keeping Instructions Fresh

- The Angular AI team updates the upstream files regularly. Revisit https://next.angular.dev/ai/develop-with-ai when bumping Angular versions or adopting new control-flow features to keep `.instructions.md` aligned.
