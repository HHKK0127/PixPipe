# Nushell script for lossless image to JXL conversion
# Usage: nu jpg-to-jxl.nu --convertPath "Z:\path\to\images"

def main [--convertPath: string] {
    let img_ext = ['.jpg', '.JPG', '.jpeg', '.JPEG', '.png', '.PNG', '.gif', '.GIF', '.bmp', '.BMP', '.webp', '.WEBP', '.tiff', '.TIFF', '.tif', '.TIF']

    # Check if path exists
    if not ($convertPath | path exists) {
        print "Error: Path not found"
        exit 1
    }

    print ""
    print $"Processing directory: ($convertPath)"
    print "Searching for image files..."
    print ""

    # Get list of files to process
    let files = (glob $"($convertPath)/**/*" -D | each { |path|
        {name: $path}
    } | where { |file|
        let ext = ($file.name | path parse | get extension)
        $img_ext | contains $ext
    })

    # Process files
    let results = ($files | each { |file|
        let jpg_path = $file.name
        let base_path = ($file.name | path parse)
        let base_dir = $base_path.parent
        let file_stem = $base_path.stem
        let jxl_path = $"($base_dir)/($file_stem).jxl"

        if ($jxl_path | path exists) {
            print $"  SKIP: ($file.name | path basename) (already converted)"
            "skip"
        } else {
            print $"Processing: ($file.name | path basename)"

            let cjxl_result = (try {
                ^cjxl $jpg_path $jxl_path -d 0 out> /dev/null err> /dev/null
                true
            } catch {
                false
            })

            if $cjxl_result and ($jxl_path | path exists) {
                try {
                    rm $jpg_path
                    let jxl_name = $jxl_path | path basename
                    print $"  OK: ($file.name | path basename) to $jxl_name"
                    "success"
                } catch {
                    print $"  Error: Failed to remove original file"
                    "failed"
                }
            } else {
                print "  Retrying with ImageMagick conversion..."
                let temp_num = (random int 100000..999999)
                let temp_jpg = $"($base_dir)/($temp_num).jpg"

                let img_result = (try {
                    ^magick $jpg_path -colorspace RGB $temp_jpg out> /dev/null err> /dev/null
                    true
                } catch {
                    false
                })

                if $img_result and ($temp_jpg | path exists) {
                    let retry_result = (try {
                        ^cjxl $temp_jpg $jxl_path -d 0 out> /dev/null err> /dev/null
                        true
                    } catch {
                        false
                    })

                    if $retry_result and ($jxl_path | path exists) {
                        try {
                            rm $jpg_path
                            rm $temp_jpg
                            let jxl_name = $jxl_path | path basename
                            print $"  OK: ($file.name | path basename) to $jxl_name (after ImageMagick conversion)"
                            "success"
                        } catch {
                            print "  Error: Failed to clean up files"
                            "failed"
                        }
                    } else {
                        try {
                            rm $temp_jpg
                        } catch {}
                        print "  Error: Conversion failed even after ImageMagick retry"
                        "failed"
                    }
                } else {
                    print "  Error: ImageMagick conversion failed"
                    "failed"
                }
            }
        }
    })

    let success_count = ($results | where { $it == "success" } | length)
    let skip_count = ($results | where { $it == "skip" } | length)
    let failed_count = ($results | where { $it == "failed" } | length)

    print ""
    print "Completed"
    print $"Success: ($success_count) files"
    print $"Skipped: ($skip_count) files (already converted)"
    print $"Failed: ($failed_count) files"
}
