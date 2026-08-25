import * as vscode from 'vscode';
import * as fs from 'fs';
import * as path from 'path';
import {
  Executable,
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from 'vscode-languageclient/node';
import { formatOnSaveEnabled, getCliPath, getServerPath, lintOnSaveEnabled, setExtensionPath } from './config';
import { runSpectraCli } from './cliClient';

const RUN_DIAGNOSTICS_COMMAND = 'spectra.diagnostics.run';
const LINT_WORKSPACE_COMMAND = 'spectra.lintWorkspace';
const COMPILE_CURRENT_FILE_COMMAND = 'spectra.compileCurrentFile';
const CHECK_CURRENT_FILE_COMMAND = 'spectra.checkCurrentFile';
const RUN_CURRENT_FILE_COMMAND = 'spectra.runCurrentFile';
const COMPILER_ACTIONS_COMMAND = 'spectra.compilerActions';
const API_ACTIONS_COMMAND = 'spectra.apiActions';
const NEW_PROJECT_COMMAND = 'spectra.newProject';

let client: LanguageClient | undefined;
let outputChannel: vscode.OutputChannel | undefined;
let spectraRunTerminal: vscode.Terminal | undefined;

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  outputChannel = vscode.window.createOutputChannel('Spectra');
  context.subscriptions.push(outputChannel);

  // Propagate extensionPath to config so getCliPath() can find the bundled
  // binary (server/<platform>/spectralang.exe) even when it is not on PATH.
  setExtensionPath(context.extensionPath);

  // Register all commands first; they do not depend on LSP availability.
  registerCommands(context);
  registerFormatOnSaveHook(context);

  context.subscriptions.push(
    vscode.workspace.onDidChangeConfiguration((event) => {
      if (event.affectsConfiguration('spectra.serverPath')) {
        void restartClient(context);
      }
    })
  );

  // Link provider: makes "  --> file.spectra:line:col" clickable in the terminal.
  context.subscriptions.push(
    vscode.window.registerTerminalLinkProvider(new SpectraTerminalLinkProvider())
  );

  // Clear the terminal reference when the user closes it manually.
  context.subscriptions.push(
    vscode.window.onDidCloseTerminal((t) => {
      if (t === spectraRunTerminal) {
        spectraRunTerminal = undefined;
      }
    })
  );

  // Start LSP without blocking CLI commands if startup fails.
  try {
    client = await startClient(context, outputChannel);
  } catch {
    // Errors were already logged and reported inside startClient.
    // The extension keeps working without LSP (CLI commands remain available).
  }
}

function registerCommands(context: vscode.ExtensionContext): void {
  // NOTE: RUN_DIAGNOSTICS_COMMAND and LINT_WORKSPACE_COMMAND are NOT registered
  // here. They are advertised by execute_command_provider in the LSP server and
  // vscode-languageclient registers them automatically when the client starts.
  // Registering them manually would cause a conflict:
  // "command already exists".

  context.subscriptions.push(
    vscode.commands.registerCommand(COMPILE_CURRENT_FILE_COMMAND, async () => {
      await executeCliCommandForActiveDocument('compile', 'Compile current file');
    })
  );

  context.subscriptions.push(
    vscode.commands.registerCommand(CHECK_CURRENT_FILE_COMMAND, async () => {
      await executeCliCommandForActiveDocument('check', 'Check current file');
    })
  );

  context.subscriptions.push(
    vscode.commands.registerCommand(RUN_CURRENT_FILE_COMMAND, async () => {
      const document = await getActiveSpectraDocumentForCommand();
      if (!document) {
        return;
      }
      const cliPath = getCliPath();
      const workspaceFolder = vscode.workspace.getWorkspaceFolder(document.uri);
      runFileInTerminal(document.fileName, cliPath, workspaceFolder?.uri.fsPath);
    })
  );

  context.subscriptions.push(
    vscode.commands.registerCommand(COMPILER_ACTIONS_COMMAND, async () => {
      await showCompilerActionsQuickPick();
    })
  );

  context.subscriptions.push(
    vscode.commands.registerCommand(API_ACTIONS_COMMAND, async () => {
      await showApiActionsQuickPick();
    })
  );

  context.subscriptions.push(
    vscode.commands.registerCommand(NEW_PROJECT_COMMAND, async () => {
      await createNewProject();
    })
  );
}

export async function deactivate(): Promise<void> {
  if (client) {
    await client.stop();
    client = undefined;
  }
}

async function startClient(
  context: vscode.ExtensionContext,
  output: vscode.OutputChannel
): Promise<LanguageClient> {
  const serverPath = getServerPath(context);
  const usesPathLookup = serverPath === 'spectra-lsp';

  if (!usesPathLookup && !fs.existsSync(serverPath)) {
    const message = `Spectra language server not found at ${serverPath}. Reinstall the extension with the repository installer or configure spectra.serverPath.`;
    output.appendLine(message);
    throw new Error(message);
  }

  const executable: Executable = {
    command: serverPath,
    transport: TransportKind.stdio,
  };

  const serverOptions: ServerOptions = {
    run: executable,
    debug: executable,
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      { scheme: 'file', language: 'spectra' },
      { scheme: 'untitled', language: 'spectra' },
    ],
    outputChannel: output,
    synchronize: {
      configurationSection: 'spectra',
      fileEvents: vscode.workspace.createFileSystemWatcher('**/*.spectra'),
    },
    initializationOptions: {
      spectra: {
        cliPath: getCliPath(),
        lintOnSave: lintOnSaveEnabled(),
      },
    },
  };

  const nextClient = new LanguageClient(
    'spectra',
    'Spectra Language Server',
    serverOptions,
    clientOptions
  );

  try {
    await nextClient.start();
  } catch (error) {
    const detail = error instanceof Error ? error.message : String(error);
    output.appendLine(`Failed to start spectra-lsp: ${detail}`);
    void vscode.window.showWarningMessage(
      'Spectra: language server could not be started. LSP features (hover, go-to-definition) are unavailable. Configure spectra.serverPath if needed.'
    );
    throw error;
  }

  context.subscriptions.push(nextClient);
  output.appendLine(`Spectra language server started from ${serverPath}`);
  return nextClient;
}

async function restartClient(context: vscode.ExtensionContext): Promise<void> {
  if (!outputChannel) {
    return;
  }

  if (client) {
    await client.stop();
  }

  client = await startClient(context, outputChannel);
}

function registerFormatOnSaveHook(context: vscode.ExtensionContext): void {
  context.subscriptions.push(
    vscode.workspace.onWillSaveTextDocument((event) => {
      if (!formatOnSaveEnabled() || event.document.languageId !== 'spectra') {
        return;
      }

      const editorConfig = vscode.workspace.getConfiguration('editor', event.document.uri);
      const formattingOptions: vscode.FormattingOptions = {
        insertSpaces: editorConfig.get<boolean>('insertSpaces', true),
        tabSize: editorConfig.get<number>('tabSize', 4),
      };

      event.waitUntil(
        vscode.commands.executeCommand<vscode.TextEdit[]>(
          'vscode.executeFormatDocumentProvider',
          event.document.uri,
          formattingOptions
        )
      );
    })
  );
}

async function executeCliCommandForActiveDocument(
  command: 'compile' | 'check' | 'run',
  progressTitle: string
): Promise<void> {
  const document = await getActiveSpectraDocumentForCommand();
  if (!document) {
    return;
  }

  const cliPath = getCliPath();
  const args = [command, document.fileName];
  const workspaceFolder = vscode.workspace.getWorkspaceFolder(document.uri);

  outputChannel?.show(true);
  outputChannel?.appendLine(`▶ spectra ${args.join(' ')}`);

  try {
    const result = await vscode.window.withProgress(
      {
        location: vscode.ProgressLocation.Notification,
        title: `Spectra: ${progressTitle}`,
        cancellable: false,
      },
      async () =>
        runSpectraCli(args, {
          cliPath,
          cwd: workspaceFolder?.uri.fsPath,
        })
    );

    const stdout = result.stdout.trimEnd();
    const stderr = result.stderr.trimEnd();

    if (stdout) {
      outputChannel?.appendLine(stdout);
    }

    if (stderr) {
      outputChannel?.appendLine(stderr);
    }

    if (result.exitCode === 0) {
      const message = successMessageForCliCommand(command);
      vscode.window.showInformationMessage(message);
      return;
    }

    vscode.window.showErrorMessage(
      `Spectra '${command}' exited with code ${result.exitCode}. See the Spectra output channel.`
    );
  } catch (error) {
    const detail = error instanceof Error ? error.message : String(error);
    outputChannel?.appendLine(detail);
    vscode.window.showErrorMessage(
      `Failed to run 'spectra ${command}': ${detail}`
    );
  }
}

// ---------------------------------------------------------------------------
// Integrated terminal for interactive execution
// ---------------------------------------------------------------------------

function getOrCreateSpectraRunTerminal(cliPath: string, cwd?: string): vscode.Terminal {
  // Reuse the existing terminal if it is still open.
  if (spectraRunTerminal && vscode.window.terminals.includes(spectraRunTerminal)) {
    return spectraRunTerminal;
  }

  spectraRunTerminal = vscode.window.createTerminal({
    name: 'Spectra Run',
    cwd,
    iconPath: new vscode.ThemeIcon('play'),
    color: new vscode.ThemeColor('terminal.ansiGreen'),
    env: process.env as Record<string, string>,
  });
  return spectraRunTerminal;
}

function runFileInTerminal(filePath: string, cliPath: string, cwd?: string): void {
  const terminal = getOrCreateSpectraRunTerminal(cliPath, cwd);
  const quotedCli = `"${cliPath}"`;
  const quotedFile = `"${filePath}"`;
  // PowerShell requires the invocation operator & before a quoted executable.
  // In bash/zsh, the quoted string already works as a command.
  const callPrefix = process.platform === 'win32' ? '& ' : '';
  terminal.show(true);
  terminal.sendText(`${callPrefix}${quotedCli} run ${quotedFile}`);
}

// ---------------------------------------------------------------------------
// Terminal link provider: makes "  --> file.spectra:line:col" clickable.
// ---------------------------------------------------------------------------

interface SpectraTerminalLink extends vscode.TerminalLink {
  filePath: string;
  line: number;
  column: number;
}

class SpectraTerminalLinkProvider implements vscode.TerminalLinkProvider<SpectraTerminalLink> {
  // Pattern: "  --> /path/file.spectra:42:5" (with or without leading spaces)
  private static readonly LINK_PATTERN = /--> (.+?\.spectra):([0-9]+):([0-9]+)/;

  provideTerminalLinks(context: vscode.TerminalLinkContext): SpectraTerminalLink[] {
    const match = SpectraTerminalLinkProvider.LINK_PATTERN.exec(context.line);
    if (!match) {
      return [];
    }

    const [fullMatch, filePath, lineStr, colStr] = match;
    const startIndex = context.line.indexOf(fullMatch);

    return [
      {
        startIndex,
        length: fullMatch.length,
        tooltip: `Open ${filePath}:${lineStr}:${colStr}`,
        filePath,
        line: parseInt(lineStr, 10),
        column: parseInt(colStr, 10),
      },
    ];
  }

  async handleTerminalLink(link: SpectraTerminalLink): Promise<void> {
    const uri = vscode.Uri.file(link.filePath);
    const doc = await vscode.workspace.openTextDocument(uri);
    const editor = await vscode.window.showTextDocument(doc);
    const position = new vscode.Position(
      Math.max(0, link.line - 1),
      Math.max(0, link.column - 1)
    );
    editor.selection = new vscode.Selection(position, position);
    editor.revealRange(
      new vscode.Range(position, position),
      vscode.TextEditorRevealType.InCenter
    );
  }
}

async function getActiveSpectraDocumentForCommand(): Promise<vscode.TextDocument | undefined> {
  const editor = vscode.window.activeTextEditor;
  if (!editor || editor.document.languageId !== 'spectra') {
    vscode.window.showInformationMessage('Open a Spectra file to use compiler commands.');
    return undefined;
  }

  const document = editor.document;
  if (document.isUntitled) {
    vscode.window.showInformationMessage('Save the Spectra file before using compiler commands.');
    return undefined;
  }

  if (!document.isDirty) {
    return document;
  }

  const choice = await vscode.window.showWarningMessage(
    'Save changes before running the Spectra compiler.',
    'Save and Continue',
    'Cancel'
  );

  if (choice !== 'Save and Continue') {
    return undefined;
  }

  const didSave = await document.save();
  return didSave ? document : undefined;
}

function successMessageForCliCommand(command: 'compile' | 'check' | 'run'): string {
  switch (command) {
    case 'compile':
      return 'Spectra file compiled successfully.';
    case 'check':
      return 'Spectra file check completed without errors.';
    case 'run':
      return 'Spectra file execution completed successfully.';
  }
}

// ---------------------------------------------------------------------------
// Quick Pick: compiler actions
// ---------------------------------------------------------------------------

interface CompilerActionItem extends vscode.QuickPickItem {
  action: () => Promise<void>;
}

async function showCompilerActionsQuickPick(): Promise<void> {
  const editor = vscode.window.activeTextEditor;
  const hasSpectraFile = editor?.document.languageId === 'spectra';

  const items: CompilerActionItem[] = [
    {
      label: '$(play) Run current file',
      description: 'spectra run',
      detail: 'Compiles and runs the active .spectra file in the integrated terminal',
      action: async () => {
        const document = await getActiveSpectraDocumentForCommand();
        if (!document) {
          return;
        }
        const cliPath = getCliPath();
        const workspaceFolder = vscode.workspace.getWorkspaceFolder(document.uri);
        runFileInTerminal(document.fileName, cliPath, workspaceFolder?.uri.fsPath);
      },
    },
    {
      label: '$(check) Check current file',
      description: 'spectra check',
      detail: 'Checks types and errors without compiling',
      action: () => executeCliCommandForActiveDocument('check', 'Check current file'),
    },
    {
      label: '$(tools) Compile current file',
      description: 'spectra compile',
      detail: 'Compiles the active .spectra file',
      action: () => executeCliCommandForActiveDocument('compile', 'Compile current file'),
    },
    {
      label: '$(warning) Lint workspace',
      description: 'spectra lint',
      detail: 'Runs lint across all workspace files',
      action: async () => {
        await vscode.commands.executeCommand(LINT_WORKSPACE_COMMAND);
      },
    },
    {
      label: '$(file-code) Format document',
      description: 'spectra fmt',
      detail: 'Formats the active .spectra file',
      action: async () => {
        if (!editor || editor.document.languageId !== 'spectra') {
          vscode.window.showInformationMessage('Open a Spectra file to format.');
          return;
        }
        await vscode.commands.executeCommand('editor.action.formatDocument');
      },
    },
    {
      label: '$(add) New Project',
      description: 'spectra new',
      detail: 'Creates a new Spectra project in a folder',
      action: () => createNewProject(),
    },
    {
      label: '$(globe) API Actions',
      description: 'spectra.api',
      detail: 'Inserts handlers, routes, CORS, and middleware supported by the current surface',
      action: () => showApiActionsQuickPick(),
    },
  ];

  const filteredItems = hasSpectraFile
    ? items
    : items.filter((item) => !item.description?.startsWith('spectra run') &&
                               !item.description?.startsWith('spectra check') &&
                               !item.description?.startsWith('spectra compile') &&
                               !item.description?.startsWith('spectra fmt'));

  const selected = await vscode.window.showQuickPick(filteredItems, {
    title: 'Spectra: Compiler Actions',
    placeHolder: 'Choose an action to run',
    matchOnDescription: true,
    matchOnDetail: true,
  });

  if (selected) {
    await selected.action();
  }
}

// ---------------------------------------------------------------------------
// Quick Pick: API actions supported today
// ---------------------------------------------------------------------------

async function showApiActionsQuickPick(): Promise<void> {
  const editor = vscode.window.activeTextEditor;
  const hasSpectraFile = editor?.document.languageId === 'spectra';

  const items: CompilerActionItem[] = [
    {
      label: '$(symbol-method) Insert sync handler',
      description: 'std.api.handler',
      detail: 'Creates a function that receives Request and returns Response',
      action: () => insertSpectraSnippet([
        'public func ${1:handle}(request: std.api.http.Request) returns std.api.http.Response {',
        '    return std.api.handler.json("${2:{}}")',
        '}',
      ]),
    },
    {
      label: '$(sync) Insert async handler',
      description: 'public async func handler',
      detail: 'Creates an async handler returning Task<Response>',
      action: () => insertSpectraSnippet([
        'public async func ${1:handle}(request: std.api.http.Request) returns std.api.http.Response {',
        '    return std.api.handler.json("${2:{}}")',
        '}',
      ]),
    },
    {
      label: '$(git-branch) Insert REST router',
      description: 'std.api.routing',
      detail: 'Creates a router with a basic GET route',
      action: () => insertSpectraSnippet([
        'let ${1:router} = std.api.routing.router()',
        'let ${2:route} = std.api.routing.get(${1:router}, "${3:/health}")',
        '${0}',
      ]),
    },
    {
      label: '$(shield) Insert permissive CORS',
      description: 'std.api.cors',
      detail: 'Creates a CORS policy and middleware',
      action: () => insertSpectraSnippet([
        'let ${1:policy} = std.api.cors.permissive()',
        'let ${2:cors} = std.api.cors.middleware(${1:policy})',
        '${0}',
      ]),
    },
    {
      label: '$(layers) Insert middleware chain',
      description: 'std.api.middleware',
      detail: 'Creates a chain and executes sync middleware',
      action: () => insertSpectraSnippet([
        'let ${1:chain} = std.api.middleware.chain()',
        'let ${2:next} = std.api.middleware.use_sync(${1:chain}, ${3:middleware})',
        'let ${4:response} = std.api.middleware.execute_sync(${2:next}, ${5:request}, ${6:response})',
        '${0}',
      ]),
    },
    {
      label: '$(check) Check current file',
      description: 'spectra check',
      detail: 'Runs the existing checker on the active .spectra file',
      action: () => executeCliCommandForActiveDocument('check', 'Check current file'),
    },
    {
      label: '$(tools) Compile current file',
      description: 'spectra compile',
      detail: 'Runs the existing compiler on the active .spectra file',
      action: () => executeCliCommandForActiveDocument('compile', 'Compile current file'),
    },
    {
      label: '$(file-directory) Open spectra.api bindings',
      description: 'packages/spectra-api',
      detail: 'Opens local bindings when the workspace is the SpectraLang repository',
      action: () => openSpectraApiBindings(),
    },
  ];

  const filteredItems = hasSpectraFile
    ? items
    : items.filter((item) => item.description === 'packages/spectra-api');

  const selected = await vscode.window.showQuickPick(filteredItems, {
    title: 'Spectra: API Actions',
    placeHolder: 'Choose an action supported by the current surface',
    matchOnDescription: true,
    matchOnDetail: true,
  });

  if (selected) {
    await selected.action();
  }
}

async function insertSpectraSnippet(lines: string[]): Promise<void> {
  const editor = vscode.window.activeTextEditor;
  if (!editor || editor.document.languageId !== 'spectra') {
    vscode.window.showInformationMessage('Open a Spectra file to insert API snippets.');
    return;
  }

  await editor.insertSnippet(new vscode.SnippetString(lines.join('\n')));
}

async function openSpectraApiBindings(): Promise<void> {
  for (const folder of vscode.workspace.workspaceFolders ?? []) {
    const bindingsPath = path.join(folder.uri.fsPath, 'packages', 'spectra-api', 'src', 'bindings');
    if (!fs.existsSync(bindingsPath)) {
      continue;
    }
    const doc = await vscode.workspace.openTextDocument(
      path.join(bindingsPath, 'http.spectra')
    );
    await vscode.window.showTextDocument(doc);
    return;
  }

  vscode.window.showInformationMessage(
    'spectra.api bindings were not found in this workspace.'
  );
}

// ---------------------------------------------------------------------------
// New Project
// ---------------------------------------------------------------------------

async function createNewProject(): Promise<void> {
  const projectName = await vscode.window.showInputBox({
    title: 'New Spectra Project',
    prompt: 'Project name',
    placeHolder: 'my-project',
    validateInput: (value) => {
      if (!value.trim()) {
        return 'Project name cannot be empty.';
      }
      if (!/^[a-zA-Z0-9_-]+$/.test(value.trim())) {
        return 'Use only letters, numbers, hyphens, and underscores.';
      }
      return undefined;
    },
  });

  if (!projectName) {
    return;
  }

  const folderUris = await vscode.window.showOpenDialog({
    canSelectFiles: false,
    canSelectFolders: true,
    canSelectMany: false,
    openLabel: 'Create project here',
    title: 'Choose where to create the project',
  });

  if (!folderUris || folderUris.length === 0) {
    return;
  }

  const parentFolder = folderUris[0].fsPath;
  const projectPath = path.join(parentFolder, projectName.trim());
  const cliPath = getCliPath();
  const args = ['new', projectPath];

  outputChannel?.show(true);
  outputChannel?.appendLine(`▶ spectra ${args.join(' ')}`);

  try {
    const result = await vscode.window.withProgress(
      {
        location: vscode.ProgressLocation.Notification,
        title: `Spectra: Creating project '${projectName}'`,
        cancellable: false,
      },
      async () => runSpectraCli(args, { cliPath, cwd: parentFolder })
    );

    const stdout = result.stdout.trimEnd();
    const stderr = result.stderr.trimEnd();

    if (stdout) {
      outputChannel?.appendLine(stdout);
    }
    if (stderr) {
      outputChannel?.appendLine(stderr);
    }

    if (result.exitCode !== 0) {
      vscode.window.showErrorMessage(
        `Failed to create project '${projectName}' (code ${result.exitCode}). See the Spectra output channel.`
      );
      return;
    }

    const openChoice = await vscode.window.showInformationMessage(
      `Project '${projectName}' created successfully at ${projectPath}.`,
      'Open Folder'
    );

    if (openChoice === 'Open Folder') {
      await vscode.commands.executeCommand(
        'vscode.openFolder',
        vscode.Uri.file(projectPath),
        { forceNewWindow: false }
      );
    }
  } catch (error) {
    const detail = error instanceof Error ? error.message : String(error);
    outputChannel?.appendLine(detail);
    vscode.window.showErrorMessage(`Failed to run 'spectra new': ${detail}`);
  }
}
