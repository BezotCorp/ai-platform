import type { ReactNode } from "react";
import clsx from "clsx";
import Heading from "~/components/Heading";
import styles from "./styles.module.css";

type FeatureItem = {
  title: string;
  Svg: React.ComponentType<React.ComponentProps<"svg">>;
  description: ReactNode;
};

type FeatureQuote = {
  name: string;
  github: string;
  role: string;
  testimonial: string;
};

const FeatureList: FeatureItem[] = [
  {
    title: "Open Source",
    Svg: "/img/lock-unlocked-fill.svg",
    description: (
      <>
        Built with transparency and collaboration in mind, bcaip empowers
        developers to contribute, customize, and innovate freely.
      </>
    ),
  },
  {
    title: "Runs Locally",
    Svg: "/img/category-moving.svg",
    description: (
      <>
        BCAIP runs locally to execute tasks efficiently, keeping control in your
        hands.
      </>
    ),
  },
  {
    title: "Extensible",
    Svg: "/img/category-ETF.svg",
    description: (
      <>
        Customize BCAIP with your preferred LLM and enhance its capabilities by connecting it to any
        external MCP server or API.
      </>
    ),
  },
  {
    title: "Autonomous",
    Svg: "/img/pay-in-four.svg",
    description: (
      <>
        BCAIP independently handles complex tasks, from debugging to deployment,
        freeing you to focus on what matters most.
      </>
    ),
  },
];

const FeatureQuotes: FeatureQuote[] = [
  {
    name: "Bezot Rémi",
    github: "https://github.com/bezot-remi",
    role: "Administrator",
    testimonial:
      "With BCAIP, I hope everyone can benefit from its capabilities.",
  },
];

function Feature({ title, Svg, description }: FeatureItem) {
  return (
    <div className={clsx("col col--3")}>
      <div className="text--left padding-horiz--md">
        <Svg className={styles.featureIcon} role="img" />
      </div>
      <div className="text--left padding-horiz--md">
        <Heading as="h3">{title}</Heading>
        <p>{description}</p>
      </div>
    </div>
  );
}

function Quote({ name, github, role, testimonial }: FeatureQuote) {
  return (
    <div className="col col--6">
      {/* inline styles in the interest of time */}
      <div
        className="text--left padding-horiz--md padding-bottom--xl"
        style={{
          display: "flex",
          flexDirection: "column",
          justifyContent: "center",
          alignItems: "left",
        }}
      >
        <div className="avatar">
          <img
            className="avatar__photo"
            src={`https://github.com/${github.split("/").pop()}.png`}
            alt={`${name}'s profile picture`}
          />
          <div className="avatar__intro">
            <div className="avatar__name">{name}</div>
            <small className="avatar__subtitle">{role}</small>
          </div>
        </div>
        <p>{testimonial}</p>
      </div>
    </div>
  );
}

export default function HomepageFeatures(): ReactNode {
  return (
    <section className={styles.features}>
      <div className="container">
        <div className="row">


          {FeatureList.map((props, idx) => (
            <Feature key={idx} {...props} />
          ))}

          {/* Testimonials Section */}
          <div style={{ display: "flex", flexDirection: "column", marginTop: "60px" }}>
            <h3 style={{ textAlign: "center", marginBottom: "40px" }}>Loved by engineers</h3>
            <div style={{ display: "flex", flexWrap: "wrap" }}>
              {FeatureQuotes.map((props, idx) => (
                <Quote key={idx} {...props} />
              ))}
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}
