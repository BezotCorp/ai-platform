import Link from "~/components/Link";
import { IconDownload } from "~/components/icons/download";

const DesktopInstallButtons = () => {
  return (
    <div>
      <p>Click one of the buttons below to download BCAIP Desktop for macOS:</p>
      <div className="pill-button" style={{ display: 'flex', gap: '0.5rem', flexWrap: 'wrap' }}>
        <Link
          className="button button--primary button--lg"
          to="https://github.com/BezotCorp/ai-platform/releases/download/stable/BCAIP.zip"
        >
          <IconDownload /> macOS Silicon
        </Link>
        <Link
          className="button button--primary button--lg"
          to="https://github.com/BezotCorp/ai-platform/releases/download/stable/BCAIP_intel_mac.zip"
        >
          <IconDownload /> macOS Intel
        </Link>
      </div>
    </div>
  );
};

export default DesktopInstallButtons;
