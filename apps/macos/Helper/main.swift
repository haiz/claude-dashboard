import Foundation

let args = CommandLine.arguments.dropFirst()

guard let command = args.first else {
    fputs("""
    Usage: claude-dashboard-helper <command>

    Commands:
      decrypt    Decrypt accounts and output JSON to stdout
      sync       Scan installed browsers for Claude sessions and save to accounts
      usage      Fetch usage JSON for an account (args: <orgId> <sessionKey>)
      add-key    Add or repair one account from a session key on stdin

    """, stderr)
    exit(1)
}

let rest = Array(args.dropFirst())

switch command {
case "decrypt":
    exit(DecryptCommand.run())
case "sync":
    exit(SyncCommand.run(env: .live))
case "usage":
    exit(UsageCommand.run(args: rest))
case "add-key":
    exit(AddKeyCommand.run())
default:
    fputs("Unknown command: \(command)\n", stderr)
    exit(1)
}
