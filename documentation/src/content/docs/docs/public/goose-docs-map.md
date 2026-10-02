# goose Documentation Map

> Auto-generated. Last updated: 2026-10-02

## Getting Started

### [Install goose](docs/getting-started/installation.md)

* Set LLM Provider
* Update Provider
* Running goose
* Shared Configuration Settings
* Pin a goose version in CI/CD
* Generate manpages for Linux distributions
* Additional Resources

### [Configure LLM Provider](docs/getting-started/providers.md)

* Available Providers
  * CLI Providers
  * ACP Providers
  * Z.AI Coding Plan
* Configure Provider and Model
  * Using Custom OpenAI Endpoints
    * Configuration Parameters
    * Example Configurations
    * Setup Instructions
* Configure Custom Provider
  * Command-Based Authentication
* Using goose for Free
  * Groq
  * EmpirioLabs AI
  * FuturMix
  * Novita AI
  * Routstr
  * SayGM
  * Google Gemini
  * Local LLMs
* OpenRouter Advanced Parameters
* GitHub Copilot Authentication
* Azure OpenAI Authentication
* Multi-Model Configuration
* Meta Muse Spark Reasoning Effort
* Gemini 3 Thinking Levels
* Viewing Model Reasoning

### [Using Extensions](docs/getting-started/using-extensions.md)

* Built-in Extensions
  * Built-in Platform Extensions
  * Toggling Built-in Extensions
* Discovering Extensions
* Adding Extensions
  * MCP Servers
  * Deeplinks
  * Config Entry
    * Remote extensions with a pre-registered OAuth client
* Enabling/Disabling Extensions
  * Set Default Extensions for New Sessions
  * Change Extensions Mid-Session
* Automatically Enabled Extensions
  * Automatic Detection
    * goose Prompt
    * goose Output
    * goose Prompt
    * goose Output
  * Direct Request
    * goose Prompt
    * goose Output
    * goose Prompt
    * goose Output
* Updating Extension Properties
* Removing Extensions
* Starting Session with Extensions
  * Built-in Extensions
  * External Extensions
    * Environment Variables
  * Remote Extensions over Streamable HTTP
  * Extensions in Containers
* Developing Extensions

## Guides

### [ACP Providers](docs/guides/acp-providers.md)

* Available ACP Providers
  * Amp ACP
  * Claude ACP
  * Codex ACP
  * Pi ACP
* Setup Instructions
  * Amp ACP
  * Claude ACP
  * Codex ACP
  * Pi ACP
* Usage Examples
  * Basic Usage
  * Using with Extensions
* Configuration Options
  * Amp ACP Configuration
  * Claude ACP Configuration
  * Codex ACP Configuration
  * Pi ACP Configuration
* Error Handling

### [goose Extension Allowlist](docs/guides/allowlist.md)

* How It Works
* Configuration
  * 1. Create and Deploy Allowlist
    * Example
  * 2. Set Environment Variable
* Best Practices
* Troubleshooting

### [Azure AI Foundry](docs/guides/azure-foundry-provider.md)

* Configuration
* Authentication
* Protocol routing
* Model metadata and pricing
* Troubleshooting
  * 401 or 403
  * No deployments are listed
  * Wrong protocol for a custom deployment name

### [CLI Providers](docs/guides/cli-providers.md)

* Why Use CLI Providers?
  * Benefits
    * Session Management
    * Workflow Integration  
    * Interface Consistency
* Available CLI Providers
  * Claude Code
  * OpenAI Codex
  * Cursor Agent
  * Gemini CLI
* Setup Instructions
  * Claude Code
  * OpenAI Codex
  * Cursor Agent
  * Gemini CLI
* Usage Examples
  * Basic Usage
* Configuration Options
  * Claude Code Configuration
  * Cursor Agent Configuration
  * OpenAI Codex Configuration
  * Gemini CLI Configuration
* How It Works
  * System Prompt Filtering
  * Message Translation
  * Response Processing
* Error Handling

### [Codebase Analysis](docs/guides/codebase-analysis.md)

    * Function Definition
    * Incoming Calls (Functions that call authenticate)
    * Outgoing Calls (Functions that authenticate calls)
* Analysis Modes
  * Understanding Project Organization
  * Inspecting a File
  * Tracking a Symbol Across Files
* Common Parameters
* Best Practices
  * Handling Large Outputs
  * Performance Tips

### [Configuration Files](docs/guides/config-files.md)

* Configuration Files
* Provider Configuration
* Global Settings
* Example Configuration
* Extensions Configuration
  * Tool Filtering
* Search Path Configuration
* Observability Configuration
* Recipe Command Configuration
* Configuration Priority
* Security Considerations
* Updating Configuration
* See Also

### [Custom Agents](docs/guides/context-engineering/custom-agents.md)

* Create an Agent File
* Use an Agent
  * List available agents
  * Delegate work to an agent
  * Load an agent's instructions into the current conversation
* Example Agents
  * Code reviewer
  * Documentation writer
* When to Use Agents, Skills, or Recipes
  * Can custom agents be scheduled to run?
  * Do custom agents have workflows?
  * Can custom agents use skills?
  * Can custom agents run recipes?
  * Can custom agents use MCP servers?
  * Can custom agents call subagents?
  * Can one custom agent call another custom agent?

### [Hooks](docs/guides/context-engineering/hooks.md)

* Where Hooks Live
* Create a Hook
* Hook Configuration
* Supported Events
* Hook Payload
  * Tool Input Keys
* Blocking a Tool Call
  * stdout Is the Decision Channel
  * Choose What Happens When a Hook Fails
  * What goose Validates in `hooks.json`
  * What `PreToolUseResult` Reports
  * Block a Dangerous Command
* Examples
  * Notify When a Tool Fails
  * Format Files After goose Edits Them
  * React to Long-Running Commands
* Try the Example Plugin
* Disable a Hook Plugin
* Troubleshooting
  * My Hook Did Not Run
  * My Hook Timed Out or Failed
  * My Script Cannot Find `jq` or Another Command

### [Context Engineering](docs/guides/context-engineering/index.md)

### [Plugins](docs/guides/context-engineering/plugins.md)

* What Plugins Can Provide
* Plugin Structure
  * Add a Skill to a Plugin
  * Add a Hook to a Plugin
* Plugin Locations
* Install a Plugin
* Auto-Update a Plugin
* Update a Plugin Manually
* Disable a Plugin
* Plugin Formats
* When to Use Plugins, Skills, or Hooks

### [Customizing Prompt Templates](docs/guides/context-engineering/prompt-templates.md)

* How It Works
* Managing Prompt Templates
  * Available Prompt Templates
  * Template Variable Syntax
    * Escaping Template Variables
* Additional Resources

### [Custom Slash Commands](docs/guides/context-engineering/slash-commands.md)

* Create Slash Commands
* Use Slash Commands
* Limitations
* Additional Resources

### [Subagents](docs/guides/context-engineering/subagents.md)

* How to Use Subagents
* Monitoring Subagent Activity
* Internal Subagents
  * Direct Prompts
  * Recipes
* Code Review Results
  * Critical Issues Found:
  * Recommendations:
* External Subagents
* Suggested Use Cases
* Lifecycle and Cleanup
* Configuration
  * Default Settings
  * Customizing Settings in Prompts
  * Extension Control
  * Return Mode Control
* Security Constraints
  * Allowed Operations
  * Restricted Operations
* Additional Resources

### [Providing Hints to goose](docs/guides/context-engineering/using-goosehints.md)

* Creating Your Hints File
* Setting Up Hints
  * Example Global `.goosehints` File
  * Example Local `.goosehints` File
  * Nested `.goosehints` Files
* Common Use Cases
* Best Practices
* Custom Context Files
  * Configuration

### [Persistent Instructions](docs/guides/context-engineering/using-persistent-instructions.md)

* How It Works
* Configuration
* Examples
  * Simple Text Reminder
  * File-Based Instructions
* Security Guidelines
* Code Quality
  * Combining Both
* Use Cases
  * Security Guardrails
  * Environment-Specific Behavior
  * Project-Specific Workflows
  * Temporary Reminders
* Persistent Instructions vs goosehints
* Best Practices

### [Agent Skills](docs/guides/context-engineering/using-skills.md)

* Built-in Skills
* Skill Locations
* Creating a Skill
  * Skill File Structure
* Functionality
* Code Quality
* Testing
* Security
* Skills from Plugins
* Supporting Files
* Steps
* Configuration
* Verification
* Common Use Case Examples
* Pre-deployment
* Deploy
* Rollback
* Unit Tests
* Integration Tests
* Running Tests
* Authentication
* Common Operations
  * Create a customer
  * Handle webhooks
* Error Handling
* Best Practices
* Additional Resources

### [Custom Distributions](docs/guides/custom-distributions.md)

* What you can customize
* Getting started
* Quick example: ship goose with a local model

### [Customizing the Sidebar](docs/guides/desktop-navigation.md)

* Style
* Position
* Mode
* Customize Items
* Toggle Sidebar

### [Environment Variables](docs/guides/environment-variables.md)

* Model Configuration
  * Basic Provider Configuration
  * Advanced Provider Configuration
  * Claude Thinking Configuration
  * Provider Retries
    * AWS Bedrock
    * Databricks
* Session Management
  * Model Context Limit Overrides
* Tool Configuration
* Security and Privacy
* Network Configuration
  * OAuth Callback Port
  * HTTP Proxy
* Observability
  * Observability Configuration
  * Langfuse Integration
* goose ACP Server
* Recipe Configuration
* Documentation Configuration
* Development & Testing
* Variables Controlled by goose
  * Customizing Shell Behavior
  * Using Session IDs in Workflows
* Environment Variable Passthrough
* Enterprise Environments
* Notes

### [File Access and Management](docs/guides/file-management.md)

* File Access
  * Quick File Search in goose Desktop
* File Management Best Practices
  * Version Control
  * Validation and Testing
  * Change Review
  * Codebase Organization

### [CLI Commands](docs/guides/goose-cli-commands.md)

* Flag Naming Conventions
  * Core Commands
    * help
    * configure
    * info [options]
    * version
    * update [options]
    * completion
  * Session Management
    * session [options]
    * session list [options]
    * session remove [options]
    * session rename [options]
    * session export [options]
    * session diagnostics [options]
  * Task Execution
    * run [options]
    * review [options] [range]
    * recipe
    * plugin
    * skills
    * local-models
    * schedule
    * mcp
    * acp
    * serve [options]
  * Terminal Integration
    * term
    * @goose / @g
* Interactive Session Features
  * Slash Commands
  * Themes
* Navigation and Controls
  * Keyboard Shortcuts
  * External Editor Mode
  * Command History Search

### [Set LLM Rate Limits](docs/guides/handling-llm-rate-limits-with-goose.md)

### [Rich Interactive Chat with MCP Apps](docs/guides/interactive-chat/index.md)

### [Using MCP Apps](docs/guides/interactive-chat/mcp-ui.md)

  * Launching Apps in Standalone Windows
    * Import an HTML App
  * Using Apps in Chat Windows
* For Extension Developers

### [goose Logging System](docs/guides/logs.md)

* Command History
* Session Records
* System Logs
  * Desktop Application Log
  * CLI Logs
  * Server Logs
  * LLM Request Logs

### [Adjusting Tool Output Verbosity](docs/guides/managing-tools/adjust-tool-output.md)

  * Toggle Parameter Truncation

### [Code Mode](docs/guides/managing-tools/code-mode.md)

* How Code Mode Works
  * Traditional vs. Code Mode Tool Calling
* Additional Resources

### [goose Permission Modes](docs/guides/managing-tools/goose-permissions.md)

* Permission Modes
* Configuring goose mode
* CLI Provider Permission Integration

### [Managing Tools](docs/guides/managing-tools/index.md)

### [Managing Tool Permissions](docs/guides/managing-tools/tool-permissions.md)

* Understanding Tools and Extensions
* Permission Levels
* Configuring Tool Permissions
* Benefits of Permission Management
* Example Permission Configuration
  * Task-Based Configuration

### [MCP Elicitation](docs/guides/mcp-elicitation.md)

* How MCP Elicitation Works
* For Extension Developers

### [MCP Roots](docs/guides/mcp-roots.md)

* How MCP Roots Works
* Using MCP Roots
* What Extensions Use It For
* Current Limitations
* For Extension Developers

### [Multi-Model Configuration](docs/guides/multi-model/index.md)

### [Offline / Air-gapped Docs](docs/guides/offline-docs.md)

* Docs layout
* Building a local docs root
* Configuring goose
* Notes

### [Recipes](docs/guides/recipes/index.md)

### [Recipe Reference Guide](docs/guides/recipes/recipe-reference.md)

* Recipe File Format
* Recipe Location
* Core Recipe Schema
* Field Specifications
  * Activities
    * Activity Types
    * Parameter Substitution
    * Example Configuration
  * Extensions
    * Extension Schema
    * Extension Types
    * Example Extensions Configuration
    * Extension environment variables
  * Parameters
    * Parameter Schema
    * Parameter Requirements
    * Input Types
    * Parameter Substitution in Desktop
  * Response
    * Response Schema
    * Basic Structure
    * Simple Example
  * Retry
    * Retry Schema
    * Success Check Configuration
    * How Retry Logic Works
    * Basic Retry Example
    * Advanced Retry Example
    * Environment Variables
  * Settings
    * Settings Schema
    * Understanding max_turns
    * Example Settings Configuration
  * Subrecipes
    * Subrecipe Schema
    * Example Subrecipe Configuration
* Desktop Metadata Fields
* Template Support
  * Escaping Template Variables
  * Template Inheritance
  * indent() Filter For Multi-Line Values
  * Built-in Parameters
* Validation Rules
  * Recipe-Level Validation
  * Parameter Validation
* Complete Recipe Example
* Error Handling
  * Retry-Specific Errors
* Learn More

### [Reusable Recipes](docs/guides/recipes/session-recipes.md)

* Create Recipe
* Edit Recipe
* Use Recipe
* Validate Recipe
* Share Recipe
  * Share via Recipe Link
  * Share via Recipe File
* Schedule Recipe
* Core Components
* Advanced Features
  * Automated Retry Logic
  * Structured Output for Automation
* What's Included
* Learn More

### [Saving Recipes](docs/guides/recipes/storing-recipes.md)

* Understanding Recipe Storage
  * Recipe Storage Locations
* Storing Recipes
  * Importing Recipes
* Finding Available Recipes
* Using Saved Recipes

### [Subrecipes For Specialized Tasks](docs/guides/recipes/subrecipes.md)

* How Subrecipes Work
  * Parameter Handling
* Examples
  * Sequential Processing
  * Conditional Processing
  * Context-Based Parameter Passing
* Best Practices
* Learn More

### [Running a Remote goose Server](docs/guides/remote-goose-server.md)

* Initial Setup
  * 1. Start the `goose serve` server
  * 2. Verify the server is up
  * 3. Optionally find the certificate fingerprint
  * 4. Configure goose Desktop
* Running `goose serve` as a Background Service (macOS)
* Troubleshooting
  * Server only accepts local connections
  * TLS is not enabled
  * Client cannot authenticate (401 / Unauthorized)
  * Certificate fingerprint mismatch
* Related

### [Roaming Agents](docs/guides/roaming-agents.md)

* The core idea: roaming is an ACP transport
* How it works: cards and mutual acceptance
* Using the CLI
  * Quick start
  * One-shot delegation
  * Bridging to any ACP client
* Embedding roaming in your own app
* The web client: a reference browser client
* Saved peers
* Controlling who can connect
* Letting the agent reach other agents
* Notes and limits

### [Running Tasks](docs/guides/running-tasks.md)

* Basic Usage
  * Text in the command
  * Using an instruction file
  * With stdin
    * Simple echo pipe
    * Multi-line instructions
* Key Features
  * Interactive Mode
  * Session Management
  * Set Provider and Model
  * Working with Extensions
  * Debug Mode
  * JSON Output Format
* Common Use Cases
  * Running Script Files
  * Quick Commands
  * Development Workflows
  * Combining Options

### [Adversary Mode](docs/guides/security/adversary-mode.md)

* How It Works
* Enabling Adversary Mode
* Writing Good Rules
* What Gets Reviewed
* See Also

### [Classification API Specification](docs/guides/security/classification-api-spec.md)

* Security & Privacy Considerations
* Endpoint
  * POST /
    * Request
    * Response
    * Status Codes
    * Example

### [Staying Safe with goose](docs/guides/security/index.md)

### [Prompt Injection Detection](docs/guides/security/prompt-injection-detection.md)

* How Detection Works
* Enabling Detection
  * Configuring Detection Threshold
* Enhanced Detection with Machine Learning
    * Self-Hosting ML Detection Endpoints
* See Also

### [In-Session Actions](docs/guides/sessions/in-session-actions.md)

* Edit Message
  * Edit in Place
  * Fork Session
  * Editing Scenario Tips
* Queue Messages
* Interrupt Task
* Voice Dictation
* Spellcheck
* Share Files in Session
* Mid-Session Changes

### [Managing Sessions](docs/guides/sessions/index.md)

### [Session Management](docs/guides/sessions/session-management.md)

* Start Session 
* Name Session
* Exit Session
* Search Sessions
* Resume Session
* Duplicate Sessions
* Delete Sessions
* Import Sessions
* Export Sessions

### [Smart Context Management](docs/guides/sessions/smart-context-management.md)

* How goose Manages Context
* Automatic Compaction
  * Manual Compaction
* Context Limit Strategies
* Maximum Turns
* Token Usage
* Model Context Limit Overrides
* Credit Balance Monitoring
* Cost Tracking

### [VMware Tanzu Platform](docs/guides/tanzu-ai-services.md)

* Prerequisites
* Step 1: Check Available Plans
* Step 2: Create a Service Instance
  * Option A: Single-Model Plan
  * Option B: Multi-Model Plan
* Step 3: Create a Service Key
  * Single-Model Plan Output
  * Multi-Model Plan Output
* Step 4: Identify Your Endpoint and API Key
* Step 5: Configure goose
* Step 6: Select a Model
* Troubleshooting
  * "Could not contact provider" / 401 Unauthorized on models endpoint
  * Verify your endpoint manually
  * Streaming
  * Model not found
  * Cleaning up

### [VMware Tanzu Platform - CLI Testing Guide](docs/guides/tanzu-cli-testing-guide.md)

* Prerequisites
* Locate the CLI Binary
* Test 1: Configure VMware Tanzu Platform Provider
* Test 2: Start a Session (Single-Model Plan)
* Test 3: Start a Session (Multi-Model Plan)
* Test 4: Verify Streaming
* Test 5: Verify Dynamic Model Fetching
* Test 6: Verify Error Messages
  * Missing API Key
  * Missing Endpoint
  * Wrong Endpoint
* Test 7: Switch Between Plans
* Quick Curl Verification

### [Terminal Integration](docs/guides/terminal-integration.md)

* Setup
* Usage
* Named Sessions
* Default Handler
* Show Context Status in Your Prompt
* Shell Completion for goose Commands
* Troubleshooting

### [Quick goose Tips](docs/guides/tips.md)

  * goose works on your behalf
  * Prompt goose using natural language
  * Extend goose's capabilities to any application
  * Choose how much control goose has
  * Choose the right LLM
  * Keep sessions short
  * Use Quick Launcher for faster session starts
  * Turn off unnecessary extensions or tool
  * Teach goose your preferences
  * Protect sensitive files
  * Version Control
  * Control which extensions goose can use
  * Set up starter templates
  * Embrace an experimental mindset
  * Customize the sidebar
  * Keep goose updated
  * Make Recipes Safe to Re-run
  * Add Logging to Recipes

### [Tool Shim](docs/guides/tool-shim.md)

* When to enable
* How it works
* Configuration
  * Enable the shim
  * Ollama backend (default)
  * Local backend (llama.cpp / built-in inference)
* Usage examples
* Environment variable reference
* Troubleshooting

### [Updating goose](docs/guides/updating-goose.md)

### [Anonymous Usage Data](docs/guides/usage-data.md)

* Usage data collected
* Change Your Preference

---

> Full docs: https://goose-docs.ai/
