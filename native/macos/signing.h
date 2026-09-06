#import <Foundation/Foundation.h>
#include <xpc/xpc.h>

// Verify the installed bundle and derive an exact local code-directory requirement.
// Distribution trust (Developer ID / notarization) is a separate release gate.
NSString *tr_code_requirement(NSURL *bundle, NSString *identifier);
NSURL *tr_executable_bundle(void);
