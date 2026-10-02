export const GooseLogo = (props: { className?: string }) => {
  return (
    <span className={props.className}>
      <img
        src="/img/goose-logo-black.png"
        alt="goose logo"
        className="goose-logo goose-logo--light"
        style={{ height: "auto", maxWidth: "100%" }}
      />
      <img
        src="/img/goose-logo-white.png"
        alt="goose logo"
        className="goose-logo goose-logo--dark"
        style={{ height: "auto", maxWidth: "100%" }}
      />
    </span>
  );
};
