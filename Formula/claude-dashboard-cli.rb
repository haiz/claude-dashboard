class ClaudeDashboardCli < Formula
  desc "Terminal dashboard for Claude.ai token usage"
  homepage "https://github.com/haiz/claude-dashboard"
  url "https://github.com/haiz/claude-dashboard/releases/download/v1.2.1/claude-dashboard-cli.tar.gz"
  sha256 "350a74ea4c7a280889a53e8476c8ff84062ac3d10229b7d8f9cd3e3e3756af0e"
  version "1.2.1"

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
