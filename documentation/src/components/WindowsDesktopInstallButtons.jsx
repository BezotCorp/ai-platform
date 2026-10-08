import Link from "~/components/Link";
import { IconDownload } from "~/components/icons/download";

const WindowsDesktopInstallButtons = () => {
  return (
    <div>
      <p>Click one of the buttons below to download BCAIP Desktop for Windows:</p>
      <div className="pill-button" style={{ display: "flex", gap: "0.75rem", flexWrap: "wrap" }}>
        <Link
          className="button button--primary button--lg"
          to="https://github.com/BezotCorp/ai-platform/releases/download/stable/BCAIP-win32-x64.zip"
        >
          <IconDownload /> Windows
        </Link>
      </div>
    </div>
  );
};

export default WindowsDesktopInstallButtons;
