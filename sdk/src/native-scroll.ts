// Embedded into the native host, including localWeb and nested plugin frames.
// Shares the document guard with the SDK bootstrap to avoid duplicate gestures.
import {installScrollGestures} from './scroll-gestures';
installScrollGestures();
