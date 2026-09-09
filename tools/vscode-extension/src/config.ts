import * as vscode from 'vscode';
import * as fs from 'fs';
import * as path from 'path';

const SECTION = 'spectra';

function getExecutableName(baseName: string): string {
  return process.platform === 'win32' ? `${baseName}.exe` : baseName;
}

/**
 * Maps the current platform/arch to the subdirectory name used when bundling
 * multi-platform binaries inside the VSIX (e.g. server/win32-x64/).
 */
function getPlatformDir(): string {
  const platform = process.platform; // 'win32' | 'linux' | 'darwin'
  const arch = process.arch;         // 'x64' | 'arm64'
  if (platform === 'win32') return 'win32-x64';
  if (platform === 'darwin') return arch === 'arm64' ? 'darwin-arm64' : 'darwin-x64';
  return 'linux-x64';
}

function existingPath(candidates: string[]): string | undefined {
  for (const candidate of candidates) {
    if (candidate && fs.existsSync(candidate)) {
      return candidate;
    }
  }

  return undefined;
}

export function getServerPath(context: vscode.ExtensionContext): string {
  const config = vscode.workspace.getConfiguration(SECTION);
  const configuredPath = config.get<string>('serverPath', '').trim();
  if (configuredPath !== '') {
    return configuredPath;
  }

  const executable = getExecutableName('spectra-lsp');
  const platformDir = getPlatformDir();
  const bundledCandidates = [
    // Multi-platform VSIX layout: server/<platform>-<arch>/<binary>
    path.resolve(context.extensionPath, 'server', platformDir, executable),
    // Legacy / dev layout: server/<binary>
    path.resolve(context.extensionPath, 'server', executable),
    path.resolve(context.extensionPath, 'bin', executable),
  ];
  const bundled = existingPath(bundledCandidates);
  if (bundled) {
    return bundled;
  }

  const workspaceCandidates = (vscode.workspace.workspaceFolders ?? []).flatMap((folder) => [
    path.resolve(folder.uri.fsPath, 'target', 'debug', executable),
    path.resolve(folder.uri.fsPath, 'target', 'release', executable),
    path.resolve(folder.uri.fsPath, 'tools', 'vscode-extension', 'server', executable),
  ]);
  const workspaceBinary = existingPath(workspaceCandidates);
  if (workspaceBinary) {
    return workspaceBinary;
  }


  return 'spectra-lsp';
}

let _extensionPath: string | undefined;

export function setExtensionPath(extensionPath: string): void {
  _extensionPath = extensionPath;
}

export function getCliPath(): string {
  const config = vscode.workspace.getConfiguration(SECTION);
  const configured = config.get<string>('cliPath', '').trim();
  if (configured !== '') {
    return configured;
  }

  // The crate ships the binary as `spectralang(.exe)`.
  const executables = [
    getExecutableName('spectralang'),
  ];

  // Search tiers, in priority order.
  const searchDirs: string[] = [];

  // Priority 1: bundled inside the extension (server/<platform>-<arch>/)
  if (_extensionPath) {
    const platformDir = getPlatformDir();
    searchDirs.push(
      path.resolve(_extensionPath, 'server', platformDir), // multi-platform VSIX layout
      path.resolve(_extensionPath, 'server'),              // dev layout
      path.resolve(_extensionPath, 'bin'),
    );
  }

  // Priority 2: workspace builds (target/debug or target/release)
  for (const folder of vscode.workspace.workspaceFolders ?? []) {
    searchDirs.push(
      path.resolve(folder.uri.fsPath, 'target', 'debug'),
      path.resolve(folder.uri.fsPath, 'target', 'release'),
    );
  }

  for (const dir of searchDirs) {
    const found = existingPath(executables.map((executable) => path.resolve(dir, executable)));
    if (found) {
      return found;
    }
  }

  // Fallback: expect the modern binary on PATH.
  return executables[0];
}

export function lintOnSaveEnabled(): boolean {
  return vscode.workspace.getConfiguration(SECTION).get<boolean>('lintOnSave', true);
}

export function formatOnSaveEnabled(): boolean {
  return vscode.workspace.getConfiguration(SECTION).get<boolean>('formatOnSave', false);
}
