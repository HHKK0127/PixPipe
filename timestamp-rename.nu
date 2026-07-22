# Nushell script for timestamp-based file renaming
# Usage: nu timestamp-rename.nu --targetPath "Z:\path\to\process"

def main [--targetPath: string] {
    let img_ext = ['.jxl' '.jpg' '.jpeg' '.png' '.gif' '.bmp']

    # Check if path exists
    if not ($targetPath | path exists) {
        print "Error: Folder not found"
        exit 1
    }

    print ""
    print "Processing..."
    print ""

    try {
        let files = (glob $"($targetPath)/**/*" -D | each { |path| {name: $path} } |
        where { |file|
            let ext = ($file.name | path parse | get extension | str downcase)
            let stem = ($file.name | path parse | get stem)
            ($img_ext | contains $ext) and not ($stem =~ '^[0-9]{14}')
        })

        $files | each { |file|
            let base_path = ($file.name | path parse)
            let base_dir = $base_path.parent
            let ext = $base_path.extension
            let timestamp = ($file.modified | format date '%Y%m%d%H%M%S')

            # Generate candidate names: first the base, then with random suffixes
            let candidates = (
                [
                    $"($timestamp)($ext)",
                    $"($timestamp)1($ext)",
                    $"($timestamp)2($ext)",
                    $"($timestamp)3($ext)",
                    $"($timestamp)4($ext)",
                    $"($timestamp)5($ext)",
                    $"($timestamp)6($ext)",
                    $"($timestamp)7($ext)",
                    $"($timestamp)8($ext)",
                    $"($timestamp)9($ext)"
                ]
            )

            # Find the first candidate that doesn't exist
            let new_name = (
                $candidates |
                where { |name| not ($"($base_dir)/($name)" | path exists) } |
                first
            )

            if ($new_name == null) {
                # If all candidates exist, try random numbers
                let random_num = (random int 100..9999)
                let final_name = $"($timestamp)($random_num)($ext)"

                if not ($"($base_dir)/($final_name)" | path exists) {
                    try {
                        print $">> Renaming: ($file.name | path basename) -> $final_name"
                        mv $file.name $"($base_dir)/($final_name)"
                    } catch {
                        print $"Error: Failed to rename ($file.name | path basename)"
                    }
                } else {
                    print $"SKIP: Cannot find unique name for ($file.name | path basename)"
                }
            } else {
                try {
                    print $">> Renaming: ($file.name | path basename) -> $new_name"
                    mv $file.name $"($base_dir)/($new_name)"
                } catch {
                    print $"Error: Failed to rename ($file.name | path basename)"
                }
            }
        }
    } catch {
        print "Error: Failed to process directory"
        exit 1
    }

    print ""
    print "Timestamp rename completed!"
}
