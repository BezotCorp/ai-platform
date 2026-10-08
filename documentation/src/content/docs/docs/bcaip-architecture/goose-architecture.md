---
title: BCAIP Architecture

sidebar:
  order: 1
---

# BCAIP Architecture

BCAIP, an open source AI Agent, builds upon the basic interaction framework of Large Language Models (LLMs), which primarily functions as a text-based conversational interface. It processes text input and generates text output. This "text in, text out" approach is enhanced with tool integrations, which allows the AI agent to complete tasks, creating BCAIP.

## BCAIP Components

BCAIP operates using three main components, the **interface**, the **agent**, and the **connected [extensions](/getting-started/using-extensions)**.

- **Interface**: This is the desktop application or CLI that the user is using to run BCAIP. It collects input from the user and displays outputs to the user

- **Agent**: The agent runs BCAIP's core logic, managing the interactive loop.

- **Extensions**: Extensions are components that provide specific tools and capabilities for the agent to use. These tools enable BCAIP to perform actions such as running commands and managing files.

In a typical session, the interface spins up an instance of the agent, which then connects to one or more extensions simultaneously. The interface can also create multiple agents to handle different tasks concurrently. Extensions and the interactive loop are important parts of BCAIP's functionality. The next sections will explain how BCAIP connects to extensions and processes user requests.

## Interoperability with Extensions

[Model Context Protocol (MCP)](https://modelcontextprotocol.io/) is an open standard that allows for interoperability between data sources and AI agents. BCAIP utilizes MCP to connect to [MCP systems/servers](https://github.com/modelcontextprotocol/servers?tab=readme-ov-file#model-context-protocol-servers). In BCAIP, these systems/servers are referred to as extensions.

Extensions expose their functionality to bcaip through tools. Tools are the functions that allow extensions to perform specific actions, such as running commands, or performing file operations. For example, the Google Drive extension includes a tool for searching documents. That tool is what gives bcaip the ability to perform that action.

bcaip comes with a set of [built-in extensions](/getting-started/using-extensions#built-in-extensions), each designed to enhance your interaction. These include tools for development, web scraping, automation, memory, and more. bcaip also supports [connecting to external extensions](/getting-started/using-extensions#adding-extensions) or [creating custom extensions](/tutorials/custom-extensions) as MCP servers.

To learn more about the design and implementation of extensions and tools, refer to the [Extensions Design Guide](/bcaip-architecture/extensions-design#tools).

## Agent Client Protocol (ACP)

bcaip supports the [Agent Client Protocol (ACP)](https://agentclientprotocol.com/) in two ways:

### bcaip as an ACP Server

`bcaip acp` starts bcaip as an ACP server over stdio, letting editors like JetBrains and Zed connect to it directly. See [Using bcaip in ACP Clients](/gdk/acp).

### ACP Agents as Providers

bcaip can delegate to external ACP agents (like Claude Code or Codex) as [providers](/guides/acp-providers). The ACP agent handles tool execution internally. bcaip passes configured extensions through as MCP servers.

## Interactive Loop

![interactive loop](../assets/guides/interactive-loop.png)

Let's take a closer look at the interactive loop shown above.

1. **Human Request**: The process begins and ends with you. Once you give bcaip a request, question, command, or problem to solve, the flow begins.

2. **Provider Chat**: bcaip sends your request along with a list of available tools to the [LLM provider](/getting-started/providers) you've connected. The provider processes it, and if necessary, creates a tool call as part of its response.

3. **Model Extension Call**: The LLM is capable of creating a tool call request but not able to execute it, that's when bcaip steps in. bcaip takes the tool call which is formatted in JSON, runs it, and gathers the results.

4. **Response to Model**: After executing the tool call, bcaip sends the results back to the model. If more extensions are needed, those steps will repeat.

5. **Context Revision**: bcaip will remove any old or irrelevant information, ensuring the LLM focuses solely on the information that matters the most. This helps with token management.

6. **Model Response**: Once all the tool calls are done, the LLM sends a final response back to you and restarts the loop once you respond.

## Error Handling in bcaip

As opposed to allowing an error to break the flow, bcaip captures and handles traditional errors along with execution errors. Errors such as invalid JSON, missing tools, etc. are sent back to the model as tool responses giving the LLM the information it needs to resolve the error and continue.

For more details on how bcaip handles errors, refer to the [Error Handling in bcaip](/bcaip-architecture/error-handling) Guide.

## Context Revision: Token Management

While bcaip is free and open source, there is typically a cost associated with LLM token usage. Everything competes for token usage including messages, tool requests, resources, file content, instructions, etc. This is where Content Revision comes into play to help reduce some of those costs. There are a few things that are done to assist with this:

- bcaip summarizes with faster and smaller LLMs
- bcaip includes everything versus a semantic search
- bcaip uses algorithms to delete old or irrelevant content
- bcaip will use find and replace instead of rewriting large files, use ripgrep to skip system files, and summarize verbose command outputs
