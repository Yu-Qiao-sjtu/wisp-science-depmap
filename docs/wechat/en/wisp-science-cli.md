# Wisp Science Advanced

Once you know the desktop project and conversation workflow, you may want to use Wisp in a system terminal or connect a task to scripts and logs. The standalone `wisp-science` CLI provides that entry point.

This tutorial covers CLI preparation, model environment variables, interactive mode, and one-shot tasks. Wisp uses the current directory as its workspace, so check where your terminal is before starting. For desktop SSH environments and interactive terminals, see [Server Environment Setup](wisp-science-servers-cli.md).

> API addresses, model IDs, and file paths are examples. Replace them with your actual configuration. The commands are not evidence of completed analyses.

**Check that the standalone CLI is available.**

If you have built or installed the standalone CLI and `wisp-science` is on PATH, run it from your project directory. Installing the desktop app does not necessarily put this command on the system PATH.

**Configure a model for this terminal.**

The CLI uses environment variables. It does not automatically turn desktop keyring settings into terminal environment variables. For a compatible API, replace the placeholders below with your own values.

macOS / Linux:

```bash
export WISP_PROVIDER="openai"
export WISP_API_URL="https://your-api.example.com"
export WISP_MODEL="your-model-id"
read -s WISP_API_KEY
export WISP_API_KEY
wisp-science
```

After `read -s WISP_API_KEY`, type the key and press Enter. Input is not echoed. The URL and model ID above are placeholders, not a working connection.

Windows PowerShell:

```powershell
$env:WISP_PROVIDER = "openai"
$env:WISP_API_URL = "https://your-api.example.com"
$env:WISP_MODEL = "your-model-id"
$credential = Get-Credential -UserName "api" -Message "Enter the API key in the password field"
$env:WISP_API_KEY = $credential.GetNetworkCredential().Password
wisp-science
```

Choose `openai`, `openai_responses`, or `anthropic` for `WISP_PROVIDER` according to the service's protocol.

**Continue a conversation in the project directory.**

After running `wisp-science`, enter natural-language tasks in interactive mode. For example:

> Inspect the current project's directory without changing it. List likely data files, analysis scripts, and result directories. Do not install dependencies or modify files yet.

Use `/help` for help, `/new` for a new conversation, `/compact` to compact context, and `/quit` to exit.

**Run one task or emit structured events.**

From the project directory:

```bash
wisp-science run "Read-only: list the top-level project files and identify likely data, script, and result directories"
wisp-science run --output jsonl "Read-only: check the column names and missing values in data/example.csv"
```

`jsonl` emits one structured event per line for logs or scripts. These commands still call a real model. A command starting successfully does not establish that the example path exists or that analysis will succeed.

**When running from source, check the working directory.**

Developers can run `cargo run -p wisp-cli -- run "task"` from the repository root. This uses the repository directory as the workspace by default. To analyze another project, build the CLI and run the executable from that project's directory. See [Development](../../development.md) for builds and additional parameters.

**Troubleshoot the command, configuration, and directory.**

| Symptom | Check first |
| --- | --- |
| `wisp-science` is not found | Whether the standalone CLI was built/installed and its directory is on PATH |
| CLI reports a missing model key | Whether the variables are set in this terminal, rather than only in desktop settings |
| Model request fails | Matching API address, protocol, model ID, and account permissions |
| Input file not found | Current working directory and whether example paths have been replaced |
| Python/R package unavailable | The actual interpreter and its dependency environment |

Start in a small practice directory and ask for a read-only file listing. Once the model responds and the directory is correct, try data inspection or one-shot output.

> See [CLI Development](../../development.md) and [Model Configuration](../../model-configuration.md). This tutorial reflects the implementation when written; parameters and messages may vary between versions.
