cask "claude-dashboard" do
  version "1.12.1"
  sha256 "620475b484a349e4e064ac78436752710b9475d24877a229cd9296b29cc612c5"

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
