import System from 'system';

import {runAll} from './harness.js';
import './colors.test.js';
import './format.test.js';
import './geometry.test.js';
import './burnRate.test.js';
import './model.test.js';
import './helper.test.js';

System.exit(runAll());
