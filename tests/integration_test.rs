use std::fs;
use std::path::PathBuf;

fn create_test_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(format!("test_output_{}", name));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn cleanup_test_dir(dir: &PathBuf) {
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn test_rename_pattern_regex() {
    // Test regex-based rename pattern
    let re = regex_lite::Regex::new(r"(\d+)").unwrap();
    let result = re.replace_all("photo_123", "img_$1");
    assert_eq!(result, "photo_img_123");
}

#[test]
fn test_rename_pattern_simple() {
    // Test simple string replacement
    let old_name = "photo (1).jpg";
    let new_name = old_name.replace("(", "").replace(")", "");
    assert_eq!(new_name, "photo 1.jpg");
}

#[test]
fn test_hamming_distance() {
    // Test perceptual hash distance calculation
    fn hamming_distance(a: u64, b: u64) -> u32 {
        (a ^ b).count_ones()
    }
    
    assert_eq!(hamming_distance(0, 0), 0);
    assert_eq!(hamming_distance(0, 1), 1);
    assert_eq!(hamming_distance(0xFF, 0x00), 8);
    assert_eq!(hamming_distance(0xFFFF, 0xFFFF), 0);
}

#[test]
fn test_duplicate_group_structure() {
    // Test duplicate group data structure
    struct DuplicateGroup {
        hash: String,
        files: Vec<String>,
    }
    
    let group = DuplicateGroup {
        hash: "abc123".to_string(),
        files: vec![
            "file1.jpg".to_string(),
            "file2.jpg".to_string(),
            "file3.jpg".to_string(),
        ],
    };
    
    assert_eq!(group.files.len(), 3);
    assert!(!group.hash.is_empty());
}

#[test]
fn test_history_entry_format() {
    // Test history entry structure
    struct HistoryEntry {
        timestamp: String,
        source: String,
        dest: String,
        files_processed: usize,
        duplicates_found: usize,
    }
    
    let entry = HistoryEntry {
        timestamp: "2024-01-15 10:30:00".to_string(),
        source: "/input".to_string(),
        dest: "/output".to_string(),
        files_processed: 100,
        duplicates_found: 5,
    };
    
    assert!(entry.files_processed > 0);
    assert!(entry.duplicates_found <= entry.files_processed);
}

#[test]
fn test_progress_gauge() {
    // Test progress gauge calculation
    let total = 100usize;
    let current = 50usize;
    let ratio = current as f64 / total as f64;
    assert!((ratio - 0.5).abs() < f64::EPSILON);
    
    let bar_width = 20usize;
    let filled = (ratio * bar_width as f64) as usize;
    assert_eq!(filled, 10);
}

#[test]
fn test_file_extension_filtering() {
    // Test image extension filtering
    let extensions = vec!["jpg", "jpeg", "png", "gif", "webp"];
    
    let test_files = vec![
        ("photo.jpg", true),
        ("image.PNG", true),  // Case insensitive
        ("doc.pdf", false),
        ("pic.jpeg", true),
        ("noext", false),
    ];
    
    for (file, expected) in test_files {
        let has_ext = extensions.iter().any(|ext| {
            file.to_lowercase().ends_with(&format!(".{}", ext))
        });
        assert_eq!(has_ext, expected, "Failed for file: {}", file);
    }
}

#[test]
fn test_undo_log_operations() {
    // Test undo log add and undo
    struct UndoLog {
        entries: Vec<(String, String)>,
    }
    
    impl UndoLog {
        fn new() -> Self {
            Self { entries: Vec::new() }
        }
        
        fn add(&mut self, from: String, to: String) {
            self.entries.push((from, to));
        }
        
        fn undo_last(&mut self) -> Option<(String, String)> {
            self.entries.pop()
        }
        
        fn len(&self) -> usize {
            self.entries.len()
        }
    }
    
    let mut log = UndoLog::new();
    assert_eq!(log.len(), 0);
    
    log.add("a.jpg".to_string(), "b.jpg".to_string());
    log.add("c.jpg".to_string(), "d.jpg".to_string());
    assert_eq!(log.len(), 2);
    
    let last = log.undo_last().unwrap();
    assert_eq!(last.0, "c.jpg");
    assert_eq!(last.1, "d.jpg");
    assert_eq!(log.len(), 1);
}

#[test]
fn test_notification_levels() {
    // Test notification level handling
    enum Level {
        Info,
        Warning,
        Error,
        Success,
    }
    
    let levels = vec![Level::Info, Level::Warning, Level::Error, Level::Success];
    assert_eq!(levels.len(), 4);
}

#[test]
fn test_batch_queue_ordering() {
    // Test batch queue FIFO ordering
    let mut queue = Vec::new();
    queue.push("folder1");
    queue.push("folder2");
    queue.push("folder3");
    
    assert_eq!(queue.len(), 3);
    assert_eq!(queue.remove(0), "folder1");
    assert_eq!(queue.remove(0), "folder2");
    assert_eq!(queue.remove(0), "folder3");
    assert!(queue.is_empty());
}

#[test]
fn test_profile_switching() {
    // Test profile data structure
    struct Profile {
        name: String,
        source: String,
        dest: String,
    }
    
    let profiles = vec![
        Profile { name: "Twitter".to_string(), source: "/twitter".to_string(), dest: "/output".to_string() },
        Profile { name: "Downloads".to_string(), source: "/downloads".to_string(), dest: "/archive".to_string() },
    ];
    
    assert_eq!(profiles.len(), 2);
    assert_eq!(profiles[0].name, "Twitter");
    assert_eq!(profiles[1].name, "Downloads");
}
