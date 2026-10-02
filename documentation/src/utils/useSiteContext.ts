export default function useSiteContext() {
  return {
    siteConfig: {
      title: "goose | Your open source AI agent",
      tagline:
        "your local AI agent, automating engineering tasks seamlessly",
      url: "https://goose-docs.ai/",
      baseUrl: import.meta.env.BASE_URL || "/",
    },
  };
}
