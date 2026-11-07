# menger

*A plan is a program whose leaves are prose.*

## Connect over MCP

menger is an MCP server (Streamable HTTP). You sign in with OAuth in your
browser, using the same account as the menger app; there is no key to paste.

| | URL |
|---|---|
| prod | `https://app.menger.sh/mcp` |
| test | `https://app-test.menger.sh/mcp` |

### Claude Code

```sh
claude mcp add --transport http menger https://app.menger.sh/mcp
```

Then run `/mcp` in Claude Code, choose **menger**, and choose **Authenticate**.
Your browser finishes the sign-in.

To share it with a project, put it in the project's `.mcp.json`:

```json
{
  "mcpServers": {
    "menger": { "type": "http", "url": "https://app.menger.sh/mcp" }
  }
}
```

### Claude Desktop and claude.ai

Go to **Settings → Connectors → Add custom connector**, enter
`https://app.menger.sh/mcp`, and sign in when asked.

### Any other MCP client

Add `https://app.menger.sh/mcp` as a remote (HTTP) server. The client finds
the sign-in by itself: an unauthenticated request is answered `401`, naming
`/.well-known/oauth-protected-resource/mcp`.

## What you get

Every menger method you are allowed to call is a tool: tickets, docs, media,
members, orgs and the rest. Each call runs as you, under your own grants.
