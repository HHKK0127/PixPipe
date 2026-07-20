# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Folder Sync/Backup**: Sync folders with auto-backup and watch mode
- **Keybind Customization**: View and edit keyboard shortcuts
- **CI/CD Pipeline**: GitHub Actions workflow for automated build, test, and release
- **Integration Tests**: Comprehensive test suite for core functionality
- **Changelog Generation**: This changelog file

### Changed
- Improved error handling with anyhow crate
- Enhanced logging with env_logger
- Added rayon for parallel hash processing
- Added kamadak-exif for EXIF metadata display
- Added notify crate for folder watching

## [0.3.0] - 2026-07-20

### Added
- **Rename Preview**: Preview before/after file rename with p key
- **Similar Image Search**: Perceptual hash-based similar image detection
- **EXIF Metadata Display**: Show camera, date, GPS, ISO, aperture info
- **Parallel Hash Processing**: Multi-threaded SHA256 hashing with rayon
- **Progress Detail**: Show current filename during processing
- **Undo Support**: Undo file moves and renames
- **Dark/Light Mode**: 12 theme presets including Light and Sepia
- **Image Resize**: Optional resize before JXL conversion
- **Watermark Overlay**: Optional watermark addition
- **Batch Rename Pattern**: Regex-based rename with preview

### Changed
- Theme count increased from 10 to 12
- Menu items now numbered with zero-padding (01, 02, etc.)
- Japanese descriptions added to all menu items

## [0.2.0] - 2026-07-19

### Added
- **Splash Screen**: Animated startup screen
- **Help Screen**: Enhanced help with keybind reference
- **Mouse Support**: Scroll and click in menus
- **Progress Interrupt**: Press Esc to cancel processing
- **Command Palette**: Search and execute commands (Ctrl+P)
- **Notification Center**: View notification history
- **Export Report**: Export processing report to file
- **Timeline View**: Visual processing timeline
- **File Tree View**: Browse directory tree
- **Side-by-Side Diff**: Compare before/after sizes

### Changed
- Improved UI with better color themes
- Enhanced progress bar with gauge visualization

## [0.1.0] - 2026-07-18

### Added
- **Full Process Pipeline**: Move → Rename → Encode workflow
- **Rename Only**: Remove underscores and parentheses
- **Timestamp Rename**: Rename by last modified timestamp
- **Image to JXL**: Convert images to lossless JXL format
- **Hash Cache DB**: Build hash database for duplicate detection
- **Settings**: Configure paths and options
- **Batch Queue**: Process multiple folders in order
- **Profiles**: Save/load configuration profiles
- **Watch Mode**: Auto-process new files in folders
- **Statistics**: View processing history and stats
- **Duplicates**: View and manage duplicate files
- **JXL Settings**: Configure JXL quality settings
- **Size Compare**: Compare sizes before/after
- **Error Panel**: View error details for failures
- **Presets**: Quick conversion presets
- **Scheduler**: Schedule batch processing
- **History Export**: Export history to CSV/JSON
- **Theme Editor**: Customize color theme
- **Compression Graph**: Compression ratio by format
- **File Classify**: Auto-classify files by type
- **Meta Edit**: Batch edit EXIF metadata
- **Config IO**: Import/export config files
- **Plugins**: Manage conversion plugins

### Changed
- Initial release with core functionality
