cask "claude-dashboard" do
  version "1.38.0"
  sha256 "7523353cdaee8f1f73615a088c47f42e94524d2d4217d39b01b60358cf662d90"

  url "https://github.com/haiz/claude-dashboard/releases/download/v#{version}/ClaudeDashboard.app.zip"
  name "Claude Dashboard"
  desc "macOS menu bar app for monitoring Claude.ai token usage"
  homepage "https://github.com/haiz/claude-dashboard"

  depends_on macos: ">= :ventura"

  app "ClaudeDashboard.app"

  postflight do
    # App is ad-hoc signed (no Apple Developer ID / notarization yet), so
    # Gatekeeper blocks it with "could not verify ... free of malware".
    # Clear all extended attributes (quarantine, provenance, etc.) so the app
    # can launch on macOS 14+ including Sequoia (15) and Tahoe (26).
    system_command "/usr/bin/xattr",
                   args: ["-cr", "#{appdir}/ClaudeDashboard.app"]
  end

  zap trash: [
    "~/Library/Preferences/com.claude-dashboard.app.plist",
    "~/Library/Application Support/ClaudeDashboard",
  ]
end
