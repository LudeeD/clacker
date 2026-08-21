# clacker

Browse Hacker News inside Claude Code.

Your company frowns on you reading Hacker News all afternoon. Your company is
delighted when you spend all afternoon in Claude Code. These policies are in
tension, and `clacker` resolves it — Hacker News, delivered through your agent
harness, in a session that looks indistinguishable from work. Not one token is
spent, because there is no model: just a local server doing a convincing
impression of the Anthropic Messages API and a "brain" that is, in the end, a
`match` statement. Your usage graph stays flat. Your reading stays excellent.

![Browsing Hacker News inside Claude Code](demo.gif)

## Install

Requires [Claude Code](https://claude.com/claude-code) on your `PATH` — clacker
launches it for you.

```sh
git clone https://github.com/LudeeD/clacker
cd clacker
cargo install --path .
```

## Use it

```sh
clacker
```

That's the whole interface. Claude Code opens, and you talk to it normally:

| Say | Get |
| --- | --- |
| `front page` | Top stories (also `new`, `best`, `ask`, `show`, `jobs`) |
| `3` | Read story 3 from the current list |
| `comments 3` | That story's thread |
| `more` | Next page |
| `search rust async` | Search Hacker News |
| `help` | The list above |

Numbers, ordinals, and plain English all work — `3`, `the third one`, and
`open the discussion on 3` land in the same place. Anything that doesn't look
like a command becomes a search.

## How it works

Four moving parts, none of them a language model:

1. **A local server** binds an ephemeral port and speaks the Anthropic Messages
   API — `/v1/messages`, streaming SSE frames and all.
2. **Claude Code launches** with `ANTHROPIC_BASE_URL` pointed at that server, so
   every request goes to your loopback interface instead of Anthropic.
   `ANTHROPIC_API_KEY` is stripped from the environment so nothing can leak out
   to the real API.
3. **The "model"** is a pure function from transcript to reply. It reads what
   you typed, picks a tool, and formats what comes back. That's it.
4. **The tools are real.** clacker also runs as a stdio MCP server that Claude
   Code spawns itself (`clacker mcp`), fetching from the Hacker News Firebase
   and Algolia APIs. The tool calls scrolling past in your transcript are
   genuinely dispatched by the harness — they're just not decided by a model.

Your own MCP servers are left out of the session (`--strict-mcp-config`), and
only the Hacker News tools are allowed. A Hacker News session has no business
touching your codebase.

## Other commands

```sh
clacker --harness claude   # pick a harness explicitly
clacker serve              # run just the fake API, for poking at with curl
clacker mcp                # the MCP server, if you want it standalone
CLACKER_DEBUG=1 clacker    # log unhandled requests
```

## License

MIT — see [LICENSE](LICENSE).
