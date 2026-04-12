import Foundation

let args = CommandLine.arguments.dropFirst()

guard let command = args.first else {
    fputs("""
    Usage: claude-dashboard-helper <command>

    Commands:
      decrypt    Decrypt accounts and output JSON to stdout
      sync       Scan Chrome for Claude sessions and save to accounts

    """, stderr)
    exit(1)
}

switch command {
case "decrypt":
    exit(DecryptCommand.run())
case "sync":
    exit(SyncCommand.run())
default:
    fputs("Unknown command: \(command)\n", stderr)
    exit(1)
}
