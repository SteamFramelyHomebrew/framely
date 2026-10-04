import React from 'react';
import {Select as BaseSelect} from '@framely/sdk';
import {t} from './i18n';
export function Select(props:React.ComponentProps<typeof BaseSelect>){
 return <BaseSelect {...props} emptyLabel={t('全部{0}',{0:props.label})} clearLabel={t('清除选择')}/>;
}
