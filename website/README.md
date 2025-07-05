# EventCore Documentation Website

This directory contains the source for the EventCore documentation website, built with [mdBook](https://rust-lang.github.io/mdBook/).

## Local Development

1. Install mdBook:
   ```bash
   cargo install mdbook
   ```

2. Sync documentation from the main docs folder:
   ```bash
   ./sync-docs.sh
   ```

3. Serve the website locally:
   ```bash
   mdbook serve
   ```

4. Open http://localhost:3000 in your browser

## Building

To build the static website:

```bash
mdbook build
```

The output will be in `../target/website/`.

## Deployment

The website is automatically deployed to GitHub Pages when:
- A new release is published (not pre-releases)
- Changes are pushed to the main branch (for testing)
- Manually triggered via GitHub Actions

The deployment workflow:
1. Builds the Rust API documentation
2. Syncs the manual documentation
3. Builds the mdBook website
4. Adds version information for releases
5. Deploys to GitHub Pages

## Structure

- `src/` - Website content and pages
  - `SUMMARY.md` - Navigation structure
  - `index.md` - Homepage
  - `manual/` - User manual (synced from `/docs/manual/`)
  - `examples/` - Example documentation
- `theme/` - Custom theme files
  - `css/custom.css` - EventCore branding and styles
  - `index.hbs` - HTML template
- `static/` - Static assets
  - `logo.png` - EventCore logo
- `sync-docs.sh` - Script to sync documentation from main docs

## Customization

The website uses a custom theme based on mdBook's default theme with EventCore branding:
- Orange/yellow color scheme matching the logo
- Modern, responsive design
- Custom CSS for features grid, performance stats, and CTAs
- Dark mode support