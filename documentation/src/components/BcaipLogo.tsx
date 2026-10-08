export const BcaipLogo = (props: { className?: string }) => {
  return (
    <span className={props.className}>
      <img
        src="/img/bcaip-logo-black.png"
        alt="bcaip logo"
        className="bcaip-logo bcaip-logo--light"
        style={{ height: "auto", maxWidth: "100%" }}
      />
      <img
        src="/img/bcaip-logo-white.png"
        alt="bcaip logo"
        className="bcaip-logo bcaip-logo--dark"
        style={{ height: "auto", maxWidth: "100%" }}
      />
    </span>
  );
};
