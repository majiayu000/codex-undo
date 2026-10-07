# HEAD formula for review; stable bottles/sha256 must use actual released artifacts.
class CodexUndo < Formula
  desc "Local file checkpoints and safe restore for official Codex hooks"
  homepage "https://github.com/majiayu000/codex-undo"
  head "https://github.com/majiayu000/codex-undo.git", branch: "main"
  license "MIT"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args
  end

  test do
    assert_match "codex-undo", shell_output("#{bin}/codex-undo --version")
    system bin/"codex-undo", "install", "--hooks-file", testpath/"hooks.json"
    assert_match "codex-undo", (testpath/"hooks.json").read
    system bin/"codex-undo", "uninstall", "--hooks-file", testpath/"hooks.json"
  end
end
