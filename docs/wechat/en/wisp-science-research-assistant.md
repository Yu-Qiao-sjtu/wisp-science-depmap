# A Personal Assistant, Built for Research

You open your laptop in the morning: three projects, seven or eight conversations, and a Run that was halfway done last night. The first task is often not research but recall: where did I leave off yesterday? What comes first today? Did that differential analysis finish?

None of these questions is hard. They are just scattered, and you ask them every day.

Personal agents are shifting from "one conversation at a time" to "an assistant that is always there": Grok Bot, Meta's Muse, and OpenAI's Dots are all chasing the same idea. Instead of a pile of chat windows, you work with an assistant that remembers you and knows what you are working on.

Wisp Science brings this to research: the **Research Assistant**.

> This article introduces a feature. Project names, conversations, and results in the examples are fictional and only illustrate the interaction.

<!-- Figure: the "Research assistant" entry in the top-right corner of the home page -->

## What it is: one conversation that never ends

Click "Research assistant" in the top-right corner of the Wisp home page. It opens a conversation that **belongs to no project, and there is never a second one**.

There is no session list and no hunting for "where was that chat last time". Every time you open it, you return to the same assistant and the same conversation. Earlier messages load page by page, and long conversations use Wisp's archive-first compaction, so old messages are never silently dropped.

Its role is clear: **a research secretary, not research labor.** It knows what happened, remembers what you plan to do, and hands work to the right place. The actual analysis happens in project conversations.

## Four things it does

### 1. Report: what did I do yesterday?

> **You**: What did I do yesterday?
>
> **Assistant**: Two projects have records from yesterday:
> - **RNA-seq aging cohort**: QC finished, and 3 samples were flagged as low quality; the normalization comparison Run failed (out of memory).
> - **Literature review**: two new finding records on conflicting definitions of SASP markers.

The assistant reads the research activity each project has **already registered**: runs (including failed ones), output files, research records, the requests you made in conversations, and daily research reviews. It is the same data as the research calendar on the home page, just available as a one-line question.

It only reports what the records contain. It will not invent what was never registered.

### 2. Plan: what am I doing today?

> **You**: Note that today I want to finish Methods and follow up with my collaborator about the ATAC data.
>
> **Assistant**: Saved two plan items for today. One item from yesterday is still open, "Review the 3 low-quality samples", so it is on today's list too.

Plans are saved item by item, with a date and status (to do / done / dropped). Unfinished items carry over to today's list automatically.

The key point: **plans live outside the conversation**. However long the conversation grows and however many times it is compacted, the plans you stated are never "forgotten".

### 3. Delegate: start work in a project

> **You**: Open a conversation in the RNA-seq project, rerun the differential analysis with batch as a covariate, and output the top 50 genes.
>
> **Assistant**: Created the conversation "Rerun differential analysis (batch covariate)" in "RNA-seq aging cohort". The task is running in the background and has been added to today's plan.

The assistant creates a titled conversation in the target project and sends it a complete, self-contained instruction. From there, **that project's own agent** does the work, with the project's files, runtime, Skills, and server configuration. Any action that needs approval still waits for you in that project.

You do not switch projects, create a conversation, or re-explain the background.

### 4. Follow up: did that task finish?

> **You**: How is the differential analysis going?
>
> **Assistant**: Done. 312 genes have FDR < 0.05, and the top 50 genes were saved as `de_top50_batch.csv`. See the conversation "Rerun differential analysis (batch covariate)" in the RNA-seq project.

The assistant checks whether delegated conversations are still running and reads their final answers.

<!-- Figure: reporting, planning, and delegating in the Research Assistant conversation -->

## Why it "doesn't do the work"

The Research Assistant has no file read/write, no shell, no Python/R runtime, and no MCP or external agent connections. This is deliberate:

- **Clear division of labor**: the assistant handles the big picture, projects handle the details. Analysis context, outputs, and run records stay in the project they belong to, where they are traceable and reproducible.
- **Safer**: a conversation that spans every project is the last place that should be able to casually edit files or run commands.
- **No conflicts**: the same problem is never half-done in the assistant and half-done in a project.

If you ask it to "help analyze this", it asks which project the work belongs in and delegates it there.

## What it cannot do yet

To be clear about the boundaries:

- **No proactive push**: you have to ask for the morning report; it will not pop up on its own.
- **No automatic report when a delegated task finishes**: ask once, or check the project.
- **Plans do not appear on the research calendar yet.**
- **It only knows registered records**: unregistered files and ad hoc terminal commands are invisible to it.
- Projects hidden by privacy mode are invisible to the assistant and cannot receive delegated work.

## A routine you can repeat every day

- **When you start**: "What did I do yesterday? What is still unfinished?"
- **Whenever something comes to mind**: "Note that..." "In project X, have it..."
- **Before you finish**: "How are today's delegated tasks doing? Which plans are still open?"

Three questions, and the rhythm of your research day is clear.

**A personal assistant for research that helps you remember your research, not do it for you.**

Project and downloads: [Wisp Science](https://github.com/xuzhougeng/wisp-science)

Continue reading: [Research Journey](wisp-science-research-journey.md) · [Quick Start](wisp-science-quick-start.md) · [Specialists](wisp-science-specialists.md)
