export interface InstallTarget {
  targetPath: string;
  relaunchPath: string;
  // Used to confirm the extracted payload really is an app before the backup is deleted.
  executableRelativePath: string;
}