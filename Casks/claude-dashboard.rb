cask "claude-dashboard" do
  version "1.0.0"
  sha256 "PLACEHOLDER_SHA256"

  url "https://github.com/haiz/claude-dashboard/releases/download/v#{version}/ClaudeDashboard.app.zip"
  name "Claude Dashboard"
  desc "macOS menu bar app for monitoring Claude.ai token usage"
  homepage "https://github.com/haiz/claude-dashboard"

  depends_on macos: ">= :ventura"

  app "ClaudeDashboard.app"

  zap trash: [
    "~/Library/Preferences/com.claude-dashboard.app.plist",
    "~/Library/Application Support/ClaudeDashboard",
  ]
end
