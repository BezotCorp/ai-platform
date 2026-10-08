export const BCAIP_ICON_URL = new URL(
  '../../../../resources/bcaip-icon.svg',
  import.meta.url
).href;

interface BcaipWatermarkProps {
  className?: string;
}

export function BcaipWatermark({ className = '' }: BcaipWatermarkProps) {
  return (
    <div className={`pointer-events-none select-none w-[70%] ${className}`}>
      <img
        src={BCAIP_ICON_URL}
        alt=""
        aria-hidden="true"
        className="w-full h-auto"
        style={{ opacity: 0.08 }}
      />
    </div>
  );
}
