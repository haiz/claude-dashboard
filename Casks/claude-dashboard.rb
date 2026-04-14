cask "claude-dashboard" do
  version "1.2.2"
  sha256 "f66b3aadb8a9b4a32c23f8846d7a8cb98d23a5b06d8ba221cdb3d5f6d9260b78"

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
