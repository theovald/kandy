import React from "react";

// The Kandy wordmark. Rendered as SVG text in Source Sans Pro (Kantega brand
// typography) rather than hand-drawn glyph paths, so it stays crisp at any size
// and is trivial to restyle. Keeps the original component API (width / height /
// className) and the shared `logo-primary` color token, so every call site —
// the sidebar and the onboarding screens — works unchanged.
const HandyTextLogo = ({
  width,
  height,
  className,
}: {
  width?: number;
  height?: number;
  className?: string;
}) => {
  return (
    <svg
      width={width}
      height={height}
      className={className}
      viewBox="0 0 930 328"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
    >
      <text
        x="465"
        y="164"
        textAnchor="middle"
        dominantBaseline="central"
        textLength="880"
        lengthAdjust="spacingAndGlyphs"
        fontFamily="'Source Sans Pro', -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif"
        fontWeight={700}
        fontSize={280}
        className="logo-primary"
      >
        Kandy
      </text>
    </svg>
  );
};

export default HandyTextLogo;
