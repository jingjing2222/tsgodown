export async function runCompiler(_cwd: string, _json: boolean) {
  throw new Error(
    [
      "[compiler-mode] default compiler kickoff is not implemented yet",
      "source: cli-default-command",
      "stage: COMPILER_KICKOFF",
      "cause: compiler mode pipeline entrypoint missing",
      "guidance: run `tsgodown build` for current Rust-backed build flow until compiler-mode is fully implemented.",
    ].join("; "),
  );
}
