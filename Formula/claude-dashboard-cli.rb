class ClaudeDashboardCli < Formula
  desc "Terminal dashboard for Claude.ai token usage"
  homepage "https://github.com/haiz/claude-dashboard"
  url "https://github.com/haiz/claude-dashboard/releases/download/v1.13.1/claude-dashboard-cli.tar.gz"
  sha256 "c356003a27958a3b25536dc0365cf979ee76766678e1f8bf893eae679ea802ec"
  version "1.13.1"

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
