window.PRODUCT_SITE = {
  name: "FoxCull Codex",
  mark: "FX",
  kicker: "Culling plus quick edit workflows",
  headline: "Cull the shoot, then turn the chosen clips into share-ready cuts.",
  subhead: "FoxCull Codex extends the culling workflow with an edit lane for duplicate segments, crop presets, stream-copy trims, and fast H.264/AAC exports.",
  repoUrl: "https://github.com/kumaradarsh1993/FoxCullCodex",
  scene: "edit",
  theme: {
    bg: "#f5f7f2",
    ink: "#141c17",
    accent: "#2d7f5e",
    accent2: "#cf4e58",
    accent3: "#2b62b2"
  },
  downloads: [
    {
      label: "Download for Windows",
      note: "Stable v0.2.0 installer",
      href: "https://github.com/kumaradarsh1993/FoxCullCodex/releases/download/v0.2.0/FoxCull.Codex_0.2.0_x64-setup.exe"
    },
    {
      label: "Download for macOS",
      note: "Apple silicon DMG",
      href: "https://github.com/kumaradarsh1993/FoxCullCodex/releases/download/v0.2.0/FoxCull.Codex_0.2.0_aarch64.dmg"
    },
    {
      label: "Download for Linux",
      note: "AppImage",
      href: "https://github.com/kumaradarsh1993/FoxCullCodex/releases/download/v0.2.0/FoxCull.Codex_0.2.0_amd64.AppImage"
    },
    {
      label: "Portable Windows",
      note: "Zip build",
      href: "https://github.com/kumaradarsh1993/FoxCullCodex/releases/download/v0.2.0/foxcull-codex_0.2.0_x64_portable.zip"
    }
  ],
  secondary: [
    { label: "View source", href: "https://github.com/kumaradarsh1993/FoxCullCodex" },
    { label: "All releases", href: "https://github.com/kumaradarsh1993/FoxCullCodex/releases" }
  ],
  stage: {
    title: "Cull-to-edit lane",
    status: "Prepared for publish",
    rail: [["Cull", "Choose media"], ["Edit", "Crop segments"], ["Export", "Share-ready"]],
    surfaceTitle: "Segment board",
    tiles: ["clip", "crop", "timeline", "preset", "music", "color", "nvenc", "export", "folder"],
    note: "This is the experimental branch where media selection and lightweight export prep sit in the same app."
  },
  storyTitle: "From folder chaos to usable cuts",
  storyIntro: "The project keeps the speed of a culling app, then adds just enough editing to prepare clips for real output.",
  chapters: [
    {
      title: "Review and select",
      body: "Move through folders with the familiar grid, details, and focus views, then pick the files or clips worth shaping."
    },
    {
      title: "Build edit segments",
      body: "Add active videos, selected videos, or duplicate timeline sections. Use 9:16, 1:1, 16:9, or original presets."
    },
    {
      title: "Export with intent",
      body: "Use stream-copy when possible, or re-encode with crop, color, audio, and hardware acceleration when needed."
    }
  ],
  downloadTitle: "Installer links are ready",
  downloadIntro: "These links target the current release assets. The repo is private right now, so public access depends on visibility.",
  panels: [
    {
      title: "Stable",
      body: "The installer links point to the v0.2.0 release assets."
    },
    {
      title: "Private repo",
      body: "The page is prepared locally, but Google and public visitors will not see it until the GitHub repo and Pages are public."
    },
    {
      title: "Quick edit focus",
      body: "This fork emphasizes segment edits, crop frames, presets, and folder organization beyond the base culling app."
    }
  ],
  setupTitle: "Use it like a media workbench",
  setupIntro: "Install the app, open a media folder, and use the edit lane only when a selected clip needs output prep.",
  setup: [
    { title: "Install the app", body: "Use the stable installer or portable build for your machine." },
    { title: "Open a folder", body: "Review media in place and keep catalog metadata separate from the base fox-cull app." },
    { title: "Add edit segments", body: "Send selected clips into edit mode, choose presets, and adjust crop frames." },
    { title: "Export intentionally", body: "Use stream-copy for simple cuts or re-encode when crop, color, or music changes are needed." }
  ],
  footer: "Experimental cull-to-edit desktop workflow, prepared for public presentation when the repo is ready."
};
