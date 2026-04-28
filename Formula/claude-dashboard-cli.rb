class ClaudeDashboardCli < Formula
  desc "Terminal dashboard for Claude.ai token usage"
  homepage "https://github.com/haiz/claude-dashboard"
  url "https://github.com/haiz/claude-dashboard/releases/download/v1.7.0/claude-dashboard-cli.tar.gz"
  sha256 "8a42d6054e94d63d1a3f3fcf144c561b0b04a8bb664694e22a6cdc2aee8e00f1"
  version "1.7.0"

  depends_on "jq"
  depends_on :macos => :ventura

  def install
    bin.install "claude-dashboard-helper"
    bin.install "claude-dashboard-cli"
  end

  test do
    assert_match "claude-dashboard-helper", shell_output("#{bin}/claude-dashboard-helper 2>&1", 1)
    assert_match "Usage", shell_output("#{bin}/claude-dashboard-cli --help")
  end
end
