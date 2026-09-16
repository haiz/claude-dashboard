import System from 'system';

import {runAll} from './harness.js';
import './colors.test.js';
import './format.test.js';
import './geometry.test.js';
import './burnRate.test.js';
import './model.test.js';
import './state.test.js';
import './sort.test.js';
import './usageLog.test.js';
import './chart.test.js';
import './command.test.js';
import './autoRun.test.js';
import './accountMerge.test.js';
import './update.test.js';
import './helper.test.js';

System.exit(runAll());
