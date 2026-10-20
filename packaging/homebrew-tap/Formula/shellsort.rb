class Shellsort < Formula
  desc "Terminal-first macOS file organizer"
  homepage "https://github.com/a1nbuselvan/Shellsort"
  version "0.1.0"

  depends_on :macos

  on_macos do
    on_arm do
      url "REPLACE_WITH_VERSIONED_ARM64_ARCHIVE_URL"
      sha256 "REPLACE_WITH_ARM64_ARCHIVE_SHA256"
    end

    on_intel do
      url "REPLACE_WITH_VERSIONED_INTEL_ARCHIVE_URL"
      sha256 "REPLACE_WITH_INTEL_ARCHIVE_SHA256"
    end
  end

  def install
    bin.install "shellsort"
    bin.install "organizer.sh"
  end

  test do
    (testpath/"sample.txt").write("fixture")
    assert_match "Documents/sample.txt", shell_output("#{bin}/shellsort #{testpath}")
    assert_path_exists testpath/"sample.txt"
  end
end
